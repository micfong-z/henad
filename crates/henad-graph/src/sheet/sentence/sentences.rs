//! The writer of the sentences of a sheet's lines and the headings of its cards.

use std::collections::HashMap;
use std::fmt;

use super::card::{
    ALWAYS, ANY_STATE_ID, BLANK_TEXT, Blank, ConditionKind, DrawKind, LineSelect, around_words, button_card_heading,
    card_header, comparison_words, condition_head, function_words, line_word, operation_words, rate_unit_word,
    relation_words, shared_state_name,
};
use super::grammar::{lower_first, plural, possessive, thing_word, upper_first};
use super::parts::{
    ChipKind, FieldPlace, PartPlace, PlaceTarget, PlacedPart, SentenceChip, SentencePart, sentence_text,
};
use super::pickers::{ANY_STATE, INCLUDING_ME, LOOK_AT_AHEAD, LOOK_AT_HERE, LOOK_AT_PLACE};
use super::rates::for_each_words;
use crate::sheet::check::governing_property;
use crate::sheet::document::{
    Around, CardRuns, CellWhich, CompareTest, LineKind, LineMode, MadeOf, PickRule, PropertyType, SheetCard, SheetChip,
    SheetDocument, SheetEvent, SheetLine, SheetLiteral, SheetParameter, SheetProperty, SheetValue, Slot, SummaryKind,
};
use crate::sheet::relations::CardRelations;
use crate::sheet::writer::format_number;

/// Parts of a sentence being written, each with its place. Adjacent words merge into one [`SentencePart::Text`].
#[derive(Debug, Default)]
struct Parts(Vec<PlacedPart>);

impl Parts {
    /// Returns parts holding `words` alone.
    fn text(words: &str) -> Self {
        let mut parts = Self::default();
        parts.push(words);
        parts
    }

    /// Returns parts holding the lead word `words` alone.
    fn keyword(words: &str) -> Self {
        Self::part(SentencePart::Keyword(words.to_owned()))
    }

    /// Returns parts holding `words` alone, as words when `inline` holds and as a lead word otherwise. A phrase inside
    /// a slot leads nothing.
    fn lead(inline: bool, words: &str) -> Self {
        if inline {
            Self::text(words)
        } else {
            Self::keyword(words)
        }
    }

    /// Returns parts holding `part` alone.
    fn part(part: SentencePart) -> Self {
        let mut parts = Self::default();
        parts.add(part);
        parts
    }

    /// Returns parts holding `part` alone, at `target` and wanting `want`.
    fn placed(part: SentencePart, target: PlaceTarget, want: Option<Blank>) -> Self {
        Self(vec![PlacedPart {
            part,
            place: Some(PartPlace { target, want }),
        }])
    }

    /// Returns parts holding the select `select` showing `words`, placed at the field `field` of the line or phrase.
    fn select(select: LineSelect, words: &str, field: FieldPlace) -> Self {
        Self::placed(
            SentencePart::Select(select, words.to_owned()),
            field_target(field),
            None,
        )
    }

    /// Moves every part placed at `from` to `to`.
    fn retarget(&mut self, from: &PlaceTarget, to: &PlaceTarget) {
        for placed in &mut self.0 {
            if let Some(place) = &mut placed.place
                && place.target == *from
            {
                place.target = to.clone();
            }
        }
    }

    /// Returns the parts as the parts of the value in the slot `key`: each place under the slot, and each place of the
    /// value itself wanting `want` when it wants nothing yet.
    fn under(mut self, key: &str, want: Blank) -> Self {
        for placed in &mut self.0 {
            if let Some(place) = &mut placed.place {
                if place.want.is_none() && place.target.is_direct() && !matches!(placed.part, SentencePart::Select(..))
                {
                    place.want = Some(want);
                }
                place.target = std::mem::replace(&mut place.target, PlaceTarget::Slot(String::new())).under(key);
            }
        }
        self
    }

    /// Returns `groups` with `separator` between each two.
    fn join(groups: impl IntoIterator<Item = Self>, separator: &str) -> Self {
        let mut parts = Self::default();
        for (index, group) in groups.into_iter().enumerate() {
            if index > 0 {
                parts.push(separator);
            }
            parts.append(group);
        }
        parts
    }

    /// Returns the alternatives `groups` with `separator` between each two, each opening with
    /// [`SentencePart::Alternative`].
    fn alternatives(groups: impl IntoIterator<Item = Self>, separator: &str) -> Self {
        Self::join(
            groups.into_iter().map(|group| {
                let mut parts = Self::part(SentencePart::Alternative);
                parts.append(group);
                parts
            }),
            separator,
        )
    }

    /// Adds `words` at the end, merged into the last part when that part is words.
    fn push(&mut self, words: &str) {
        if words.is_empty() {
            return;
        }
        if let Some(PlacedPart {
            part: SentencePart::Text(last),
            ..
        }) = self.0.last_mut()
        {
            last.push_str(words);
        } else {
            self.0.push(PlacedPart {
                part: SentencePart::Text(words.to_owned()),
                place: None,
            });
        }
    }

    /// Adds `part` at the end, with no place.
    fn add(&mut self, part: SentencePart) {
        self.add_placed(PlacedPart { part, place: None });
    }

    /// Adds `placed` at the end.
    fn add_placed(&mut self, placed: PlacedPart) {
        match placed.part {
            SentencePart::Text(words) => self.push(&words),
            part => self.0.push(PlacedPart {
                part,
                place: placed.place,
            }),
        }
    }

    /// Adds every part of `other` at the end.
    fn append(&mut self, other: Self) {
        for placed in other.0 {
            self.add_placed(placed);
        }
    }

    /// Replaces the words of the first part with `map` of them, when the first part is words or a lead word.
    fn map_first_text(&mut self, map: fn(&str) -> String) {
        if let Some(PlacedPart {
            part: SentencePart::Text(first) | SentencePart::Keyword(first),
            ..
        }) = self.0.first_mut()
        {
            *first = map(first);
        }
    }

    /// Turns the words that open the parts into a [`SentencePart::PhraseLead`] placed at the slot that holds them,
    /// when the parts open with words.
    fn lead_at_slot(&mut self) {
        if let Some(first) = self.0.first_mut()
            && let SentencePart::Text(words) = &mut first.part
        {
            first.part = SentencePart::PhraseLead(std::mem::take(words));
            first.place = Some(PartPlace {
                target: PlaceTarget::Slot(String::new()),
                want: None,
            });
        }
    }

    /// Merges every run of adjacent words into one part.
    fn merge_text(&mut self) {
        for placed in std::mem::take(&mut self.0) {
            self.add_placed(placed);
        }
    }

    /// Returns the parts as plain text.
    fn plain(&self) -> String {
        self.0.iter().map(|placed| placed.part.text()).collect()
    }

    /// Returns the parts in order, with their places.
    fn into_placed(self) -> Vec<PlacedPart> {
        self.0
    }
}

/// Returns `parts`, the parts of a Become of a competing group below its highest line, led by the word "or".
///
/// `lines` holds the numbers of the group's lines, and the change's first word drops to lower case: "or become Dead
/// from Infected, at rate Death rate per day".
pub fn competing_parts(parts: Vec<PlacedPart>, lines: Vec<usize>) -> Vec<PlacedPart> {
    let mut led = Parts::part(SentencePart::Competing(lines));
    led.push(" ");
    let mut change = Parts(parts);
    change.map_first_text(lower_first);
    led.append(change);
    led.0
}

/// Writer of the sentences of one sheet's lines and cards, holding the sheet's names by id.
///
/// Every reference that names nothing in the sheet reads as its id, and every blank slot as [`BLANK_TEXT`].
pub struct Sentences<'a> {
    /// The sheet.
    document: &'a SheetDocument,
    /// Every property of every kind of thing, by id.
    properties: HashMap<&'a str, &'a SheetProperty>,
    /// Every parameter, by id.
    parameters: HashMap<&'a str, &'a SheetParameter>,
    /// Every event, by id.
    events: HashMap<&'a str, &'a SheetEvent>,
    /// Every line of every card, by id.
    lines: HashMap<&'a str, &'a SheetLine>,
}

/// Prints the sheet's id.
impl fmt::Debug for Sentences<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sentences")
            .field("sheet", &self.document.sheet.id)
            .finish_non_exhaustive()
    }
}

impl<'a> Sentences<'a> {
    /// Creates the writer of the sentences of `document`.
    pub fn new(document: &'a SheetDocument) -> Self {
        let mut properties = HashMap::new();
        let things = document
            .cells
            .iter()
            .flatten()
            .chain(document.agents.iter().flat_map(|kind| &kind.properties))
            .chain(document.ground.iter().flat_map(|ground| &ground.properties));
        for property in things {
            properties.entry(property.id.as_str()).or_insert(property);
        }
        let mut lines = HashMap::new();
        for card in &document.cards {
            for (id, line) in &card.line_tables {
                lines.entry(id.as_str()).or_insert(line);
            }
        }
        Self {
            document,
            properties,
            parameters: document
                .parameters
                .iter()
                .map(|parameter| (parameter.id.as_str(), parameter))
                .collect(),
            events: document.events.iter().map(|event| (event.id.as_str(), event)).collect(),
            lines,
        }
    }

    /// Returns the word for one of the kind of thing `every`: "cell" for `cells`, "ground cell" for `ground`, and an
    /// agent kind's name, such as "bird".
    pub fn thing_word(&self, every: &str) -> String {
        thing_word(self.document, every)
    }

    /// Returns the heading of `card`: "Each tick, every cell", or for a card that runs once the heading of the card of
    /// the button that runs it.
    ///
    /// A card that runs once with no button that runs it takes its id as the button's name.
    pub fn card_heading(&self, card: &SheetCard) -> String {
        let thing = self.thing_word(&card.every);
        match card.runs {
            CardRuns::EachTick => card_header(&thing),
            CardRuns::Once => {
                let button = self.document.buttons.iter().find(|button| {
                    button
                        .does
                        .iter()
                        .any(|step| step.card.as_deref() == Some(card.id.as_str()))
                });
                button_card_heading(button.map_or(card.id.as_str(), |button| button.name.as_str()), &thing)
            }
        }
    }

    /// Returns the sentence of line `line_id` as plain text, where `otherwise` holds the numbers of the higher lines
    /// that override it, empty for a line that no line overrides.
    ///
    /// A line that no card holds reads as [`BLANK_TEXT`]. The text joins the parts of [`Self::line_parts`].
    pub fn line(&self, line_id: &str, otherwise: &[usize]) -> String {
        sentence_text(&self.line_parts(line_id, otherwise))
    }

    /// Returns the parts of the sentence of line `line_id` as the card draws them, where `otherwise` holds the numbers
    /// of the higher lines that override it, empty for a line that no line overrides.
    ///
    /// A line that no card holds reads as one part of [`BLANK_TEXT`].
    pub fn line_parts(&self, line_id: &str, otherwise: &[usize]) -> Vec<SentencePart> {
        self.placed_parts(line_id, otherwise)
            .into_iter()
            .map(|placed| placed.part)
            .collect()
    }

    /// Returns the parts of the sentence of line `line_id` as [`Self::line_parts`] does, each with the place in the
    /// line that an edit of it sets.
    pub fn placed_parts(&self, line_id: &str, otherwise: &[usize]) -> Vec<PlacedPart> {
        let Some(line) = self.lines.get(line_id) else {
            return Parts::text(BLANK_TEXT).into_placed();
        };
        if line.kind.works_out_result() {
            let phrase = self.phrase(line, false);
            return match &line.name {
                Some(name) if !(line.kind == LineKind::Compare && *name == phrase.plain()) => {
                    let mut parts = Parts::part(SentencePart::ResultName(name.clone()));
                    parts.push(" = ");
                    parts.append(phrase);
                    parts.into_placed()
                }
                _ => phrase.into_placed(),
            };
        }
        let mut change = match line.kind {
            LineKind::When => {
                let mut parts = self.condition(&line.condition, true).under("condition", Blank::When);
                parts.push(":");
                return parts.into_placed();
            }
            LineKind::Repeat => {
                let mut parts = Parts::keyword("Repeat");
                parts.push(" ");
                parts.append(self.slot_parts(&line.times, None, Blank::Times, "times"));
                parts.push(" times:");
                return parts.into_placed();
            }
            LineKind::Always => return Parts::text(ALWAYS).into_placed(),
            _ => self.change(line),
        };
        if otherwise.is_empty() {
            change.map_first_text(upper_first);
            change.into_placed()
        } else {
            let mut parts = Parts::part(SentencePart::Otherwise(otherwise.to_vec()));
            parts.push(" ");
            change.map_first_text(lower_first);
            parts.append(change);
            parts.into_placed()
        }
    }

    /// Returns the words of the result line or phrase `line` after its name, such as "Alive neighbours is 3".
    pub fn result_words(&self, line: &SheetLine) -> String {
        self.phrase(line, false).plain()
    }

    /// Returns the parts of a change line with its condition, with the first letter in upper case.
    fn change(&self, line: &SheetLine) -> Parts {
        let mut parts = Parts::default();
        match line.kind {
            LineKind::Become => parts.append(self.become_parts(line)),
            LineKind::Set => parts.append(self.set_parts(line)),
            LineKind::Steer => {
                parts.append(Parts::keyword("Steer"));
                parts.push(" ");
                parts.append(self.property_part(line.property.as_deref(), Blank::Property, FieldPlace::Property));
                parts.push(" ");
                parts.append(mode_select(LineSelect::SteerMode, line.mode));
                parts.push(" ");
                parts.append(self.slot_parts(&line.value, None, Blank::Value, "value"));
                parts.push(", strength ");
                parts.append(self.slot_parts(&line.strength, None, Blank::Number, "strength"));
            }
            LineKind::KeepBetween => {
                let property = self.property_name(line.property.as_deref());
                let arrow = line
                    .property
                    .as_deref()
                    .and_then(|id| self.properties.get(id))
                    .is_some_and(|property| property.property_type == PropertyType::Arrow);
                parts.append(Parts::keyword("Keep"));
                parts.push(" ");
                parts.append(self.property_part(line.property.as_deref(), Blank::Property, FieldPlace::Property));
                if arrow {
                    // The possessive's ending after the chip: "Velocity's length", or "Pass' length".
                    let ending = possessive(&property).split_off(property.len());
                    parts.push(&format!("{ending} length"));
                }
                parts.push(" between ");
                parts.append(self.slot_parts(&line.minimum, None, Blank::LowerNumber, "minimum"));
                parts.push(" and ");
                parts.append(self.slot_parts(&line.maximum, None, Blank::UpperNumber, "maximum"));
            }
            LineKind::Move => {
                parts.append(Parts::keyword("Move"));
                parts.push(" ");
                parts.append(mode_select(LineSelect::MoveMode, line.mode));
                parts.push(" ");
                parts.append(self.slot_parts(&line.value, None, Blank::Value, "value"));
            }
            LineKind::Leave => {
                parts.append(Parts::keyword("Leave"));
                parts.push(" ");
                parts.append(self.slot_parts(&line.amount, None, Blank::Number, "amount"));
                parts.push(" on ");
                parts.append(self.property_part(
                    line.ground_property.as_deref(),
                    Blank::GroundProperty,
                    FieldPlace::GroundProperty,
                ));
            }
            LineKind::Record => {
                parts.append(Parts::keyword("Record"));
                parts.push(" a ");
                let part = match line.event.as_deref() {
                    Some(id) => SentencePart::Chip(SentenceChip::plain(
                        ChipKind::Event(id.to_owned()),
                        self.events
                            .get(id)
                            .map_or_else(|| id.to_owned(), |event| event.name.clone()),
                    )),
                    None => SentencePart::Blank(Blank::Event),
                };
                parts.append(Parts::placed(part, field_target(FieldPlace::Event), Some(Blank::Event)));
            }
            _ => parts.push(line.kind.as_str()),
        }
        let takes_condition = matches!(
            line.kind,
            LineKind::Become | LineKind::Set | LineKind::Leave | LineKind::Record
        );
        if takes_condition || !line.condition.is_blank() {
            parts.push(", ");
            parts.append(self.condition(&line.condition, false).under("condition", Blank::When));
        }
        parts
    }

    /// Returns the parts of a Become line before its condition.
    fn become_parts(&self, line: &SheetLine) -> Parts {
        let mut parts = Parts::default();
        parts.append(Parts::keyword("Become"));
        parts.push(" ");
        parts.append(self.state_part(
            line.property.as_deref(),
            line.new_state.as_deref(),
            Blank::State,
            FieldPlace::NewState,
        ));
        parts.push(" ");
        parts.add(SentencePart::From);
        parts.push(" ");
        parts.append(match line.start_state.as_deref() {
            Some(ANY_STATE_ID) => Parts::placed(
                SentencePart::Chip(SentenceChip::plain(ChipKind::AnyState, ANY_STATE.to_owned())),
                field_target(FieldPlace::StartState),
                Some(Blank::OldState),
            ),
            other => self.state_part(line.property.as_deref(), other, Blank::OldState, FieldPlace::StartState),
        });
        parts
    }

    /// Returns the parts of a Set line before its condition.
    fn set_parts(&self, line: &SheetLine) -> Parts {
        let mut parts = Parts::default();
        parts.append(Parts::keyword("Set"));
        parts.push(" ");
        // A Set over cells changes a number, and a Set over agents any property but its place.
        let (blank, value) = if self.document.world.made_of == MadeOf::Agents {
            (Blank::Property, Blank::Value)
        } else {
            (Blank::NumberProperty, Blank::Number)
        };
        parts.append(self.property_part(line.property.as_deref(), blank, FieldPlace::Property));
        parts.push(" to ");
        // A chance or a rate sets yes when it happens and no otherwise.
        let draw = matches!(
            line.value.value(),
            Some(SheetValue::Phrase(phrase)) if matches!(phrase.kind, LineKind::Chance | LineKind::Rate)
        );
        if draw {
            parts.push(&format!("{} ", literal_words(SheetLiteral::Bool(true))));
        }
        parts.append(self.slot_parts(&line.value, None, value, "value"));
        if draw {
            parts.push(&format!(", otherwise {}", literal_words(SheetLiteral::Bool(false))));
        }
        parts
    }

    /// Returns the parts of property `id`, the line's field `field`: its chip, or `blank` for `None`.
    fn property_part(&self, id: Option<&str>, blank: Blank, field: FieldPlace) -> Parts {
        let part = match id {
            Some(id) => SentencePart::Chip(SentenceChip::plain(
                ChipKind::Property(id.to_owned()),
                self.property_name(Some(id)),
            )),
            None => SentencePart::Blank(blank),
        };
        Parts::placed(part, field_target(field), Some(blank))
    }

    /// Returns the parts of the condition in `slot`: "always", "when ...", "with chance ..." or "at rate ...", or on a
    /// When line "When ...", "When with chance ..." or "When at rate ...".
    ///
    /// The places are relative to the slot, as [`Parts::under`] takes them.
    fn condition(&self, slot: &Slot, on_when_line: bool) -> Parts {
        let Some(value) = slot.value() else {
            let mut parts = Parts::default();
            if on_when_line {
                parts.append(Parts::keyword("When"));
                parts.push(" ");
            }
            parts.append(Parts::placed(
                SentencePart::Blank(Blank::When),
                PlaceTarget::Slot(String::new()),
                Some(Blank::When),
            ));
            return parts;
        };
        let (kind, body) = match value {
            SheetValue::Phrase(phrase) => match phrase.kind {
                LineKind::Always if on_when_line => {
                    let mut parts = Parts::keyword("When");
                    parts.push(&format!(" {ALWAYS}"));
                    return parts;
                }
                LineKind::Always => {
                    return Parts::placed(
                        SentencePart::Always,
                        PlaceTarget::Slot(String::new()),
                        Some(Blank::When),
                    );
                }
                LineKind::Chance => (ConditionKind::Chance, self.chance_body(phrase)),
                LineKind::Rate => (ConditionKind::Rate, self.rate_body(phrase)),
                _ => (ConditionKind::When, self.value_parts(value, None)),
            },
            _ => (ConditionKind::When, self.value_parts(value, None)),
        };
        let mut parts = Parts::part(SentencePart::ConditionWord(condition_head(kind, on_when_line)));
        parts.push(" ");
        parts.append(body);
        parts
    }

    /// Returns the words of the value in `slot`, [`BLANK_TEXT`] for a blank slot.
    ///
    /// `about` is the property that a state in the slot belongs to.
    pub fn slot(&self, slot: &Slot, about: Option<&str>) -> String {
        self.slot_parts(slot, about, Blank::Value, "").plain()
    }

    /// Returns the parts of the value in `slot`, the slot of the key `key`, or `blank` for a blank slot or an empty
    /// list.
    ///
    /// `about` is the property that a state in the slot belongs to. Every place is under the slot.
    fn slot_parts(&self, slot: &Slot, about: Option<&str>, blank: Blank, key: &str) -> Parts {
        slot.value()
            .filter(|value| !matches!(value, SheetValue::List(items) if items.is_empty()))
            .map_or_else(
                || Parts::placed(SentencePart::Blank(blank), PlaceTarget::Slot(String::new()), None),
                |value| self.value_parts(value, about),
            )
            .under(key, blank)
    }

    /// Returns the words of `value`, where `about` is the property that a state in it belongs to.
    pub fn value(&self, value: &SheetValue, about: Option<&str>) -> String {
        self.value_parts(value, about).plain()
    }

    /// Returns the parts of `value`, where `about` is the property that a state in it belongs to.
    ///
    /// A chip of the value itself is placed at the empty slot path, an item of a list at its index, and the parts of a
    /// phrase at their places in the phrase, as [`Parts::under`] takes them.
    fn value_parts(&self, value: &SheetValue, about: Option<&str>) -> Parts {
        let here = || PlaceTarget::Slot(String::new());
        match value {
            SheetValue::Literal(literal) => Parts::placed(
                SentencePart::Chip(SentenceChip::plain(ChipKind::Literal, literal_words(*literal))),
                here(),
                None,
            ),
            SheetValue::Chip(chip) => Parts::placed(SentencePart::Chip(self.chip(chip, about)), here(), None),
            SheetValue::Phrase(phrase) if phrase.kind == LineKind::Calculate => {
                let mut parts = Parts::text("(");
                parts.append(self.phrase(phrase, true));
                parts.push(")");
                if phrase.formula.as_deref().is_some_and(is_length_of_mine) {
                    parts.lead_at_slot();
                }
                parts
            }
            SheetValue::Phrase(phrase) => {
                let mut parts = self.phrase(phrase, true);
                if matches!(phrase.kind, LineKind::RandomNumber | LineKind::RandomDirection) {
                    parts.lead_at_slot();
                }
                parts
            }
            SheetValue::List(items) => Parts::join(
                items.iter().enumerate().map(|(index, item)| {
                    let mut parts = self.value_parts(item, about);
                    parts.retarget(
                        &here(),
                        &PlaceTarget::ListItem {
                            slot: String::new(),
                            index,
                        },
                    );
                    parts
                }),
                ", ",
            ),
        }
    }

    /// Returns the chip of `chip`, where `about` is the property that a state chip belongs to.
    fn chip(&self, chip: &SheetChip, about: Option<&str>) -> SentenceChip {
        let property = |id: &str, lead: &'static str| SentenceChip {
            kind: ChipKind::Property(id.to_owned()),
            lead,
            name: self.property_name(Some(id)),
        };
        match chip {
            SheetChip::Param(id) => SentenceChip::plain(
                ChipKind::Parameter(id.clone()),
                self.parameters
                    .get(id.as_str())
                    .map_or_else(|| id.clone(), |parameter| parameter.name.clone()),
            ),
            SheetChip::My(id) => property(id, "my"),
            SheetChip::MyNew(id) => property(id, "my new"),
            SheetChip::Their(id) => property(id, "their"),
            SheetChip::Relation(relation) => {
                SentenceChip::plain(ChipKind::Relation, relation_words(*relation).to_owned())
            }
            SheetChip::State(id) => self.state_chip(about, id),
            SheetChip::Result { line, part, property } => {
                let result = self.lines.get(line.as_str());
                // An unnamed Count reads as its automatic name, which reads no result and so never comes back here.
                // Another unnamed result reads as its line, since its automatic name can read this chip again.
                let base = result
                    .and_then(|result| {
                        result.name.clone().or_else(|| {
                            (result.kind == LineKind::Count)
                                .then(|| crate::sheet::edit::automatic_name(self.document, result))
                                .flatten()
                        })
                    })
                    .unwrap_or_else(|| self.line_word_of(line));
                let name = if let Some(part) = part {
                    if part == "open" {
                        format!("{base} is open")
                    } else {
                        let part = result
                            .and_then(|result| result.summaries.iter().find(|summary| summary.id == *part))
                            .map_or(part.as_str(), |summary| summary.name.as_str());
                        format!("{} {part}", possessive(&base))
                    }
                } else {
                    match property {
                        Some(property) => format!("{} {}", possessive(&base), self.property_name(Some(property))),
                        None => base,
                    }
                };
                SentenceChip::plain(ChipKind::Result(line.clone()), name)
            }
        }
    }

    /// Returns the line word of line `line_id` as its card numbers it, such as "line 2", or the id of a line that no
    /// card holds.
    fn line_word_of(&self, line_id: &str) -> String {
        self.document
            .cards
            .iter()
            .find(|card| card.line_tables.contains_key(line_id))
            .and_then(|card| CardRelations::new(self.document, card).number(line_id))
            .map_or_else(|| line_id.to_owned(), |number| line_word(None, number))
    }

    /// Returns the parts of the phrase or result line `line` after its name.
    ///
    /// `inline` is whether the phrase sits inside a slot, where an all of reads as its items joined by "and".
    fn phrase(&self, line: &SheetLine, inline: bool) -> Parts {
        match line.kind {
            LineKind::Count => {
                let mut parts = Parts::lead(inline, "count of");
                parts.push(" ");
                parts.append(self.around(line));
                if !line.whose.is_blank() {
                    parts.push(" whose ");
                    parts.append(self.whose(&line.whose, Blank::Property, "whose"));
                }
                if !line.only.is_blank() {
                    parts.push(", only those whose ");
                    parts.append(self.whose(&line.only, Blank::Property, "only"));
                }
                parts
            }
            LineKind::LookAround => self.look_around(line, inline),
            LineKind::PickCell => self.pick_cell(line),
            LineKind::LookAtCell => self.look_at_cell(line),
            LineKind::Chance => {
                let mut parts = Parts::part(SentencePart::ConditionWord(condition_head(
                    ConditionKind::Chance,
                    false,
                )));
                parts.push(" ");
                parts.append(self.chance_body(line));
                parts
            }
            LineKind::Rate => {
                let mut parts = Parts::part(SentencePart::ConditionWord(condition_head(ConditionKind::Rate, false)));
                parts.push(" ");
                parts.append(self.rate_body(line));
                parts
            }
            LineKind::RandomNumber => {
                let mut parts = Parts::text("a random number between ");
                parts.append(self.slot_parts(&line.low, None, Blank::LowerNumber, "low"));
                parts.push(" and ");
                parts.append(self.slot_parts(&line.high, None, Blank::UpperNumber, "high"));
                parts
            }
            LineKind::RandomDirection => {
                let mut parts = Parts::text("a random direction of length ");
                parts.append(self.slot_parts(&line.length, None, Blank::Number, "length"));
                parts
            }
            LineKind::Compare => self.compare(line),
            LineKind::AllOf if inline => self.inline_all_of(line),
            LineKind::AllOf | LineKind::AnyOf | LineKind::NoneOf => self.list_of(line, inline),
            LineKind::Choose => self.choose(line, inline),
            LineKind::DependingOn => self.depending_on(line, inline),
            LineKind::Calculate => self.calculate(line, inline),
            LineKind::Always => Parts::text(ALWAYS),
            _ => self.change(line),
        }
    }

    /// Returns the parts of a Look around line or phrase after its name.
    ///
    /// `inline` is whether the phrase sits inside a slot.
    fn look_around(&self, line: &SheetLine, inline: bool) -> Parts {
        // A Total or average line of a cells card is a Look around over cells with one summary.
        if let (Some(cells), [summary]) = (line.around.and_then(around_words), line.summaries.as_slice())
            && line.only.is_blank()
            && let Some(kind @ (SummaryKind::Total | SummaryKind::Average)) = summary.summary
        {
            let mut parts = Parts::select(LineSelect::TotalOrAverage, summary_word(kind), FieldPlace::Summary);
            parts.push(" of ");
            let mut value = self.whose(&summary.value, Blank::NumberProperty, "");
            value.retarget(
                &PlaceTarget::Slot(String::new()),
                &field_target(FieldPlace::SummaryValue),
            );
            parts.append(value);
            parts.push(" over ");
            parts.append(Parts::select(LineSelect::WhichCells, cells, FieldPlace::Around));
            return parts;
        }
        let mut parts = Parts::lead(inline, "look at");
        parts.push(" ");
        parts.append(self.around(line));
        if !line.only.is_blank() {
            parts.push(", only those whose ");
            parts.append(self.whose(&line.only, Blank::Property, "only"));
        }
        parts.push(": ");
        // A look at agents chooses how each summary sums them up. A look over cells writes the words alone.
        let agents = line.around == Some(Around::AgentsWithin);
        let summaries = line.summaries.iter().enumerate().map(|(index, summary)| {
            let words = summary.summary.map_or("", summary_word);
            let mut parts = if agents {
                Parts::select(LineSelect::LookSummary(index), words, FieldPlace::LookSummary(index))
            } else {
                Parts::text(if words.is_empty() { BLANK_TEXT } else { words })
            };
            if summary.summary != Some(SummaryKind::Count) {
                parts.push(" of ");
                let key = format!("summaries.{index}.value");
                parts.append(self.slot_parts(&summary.value, None, Blank::Value, &key));
            }
            parts.push(&format!(" ({})", summary.name));
            parts
        });
        parts.append(Parts::join(summaries, ", "));
        parts
    }

    /// Returns the parts of a Pick a cell line after its name.
    fn pick_cell(&self, line: &SheetLine) -> Parts {
        let open = if line.open_only == Some(true) { "open " } else { "" };
        let pick = Parts::select(
            LineSelect::Pick,
            line.pick.map_or("", PickRule::as_str),
            FieldPlace::Pick,
        );
        if line.pick == Some(PickRule::Random) {
            let mut parts = Parts::text("a ");
            parts.append(pick);
            parts.push(&format!(" {open}neighbouring cell"));
            parts
        } else {
            let mut parts = Parts::text(&format!("the {open}neighbouring cell with the "));
            parts.append(pick);
            parts.push(" ");
            parts.append(self.property_part(line.ranked_by.as_deref(), Blank::GroundProperty, FieldPlace::RankedBy));
            parts.push(" (ties at random)");
            parts
        }
    }

    /// Returns the parts of a Look at a cell line after its name.
    fn look_at_cell(&self, line: &SheetLine) -> Parts {
        let which = |words: &str| Parts::select(LineSelect::WhichCell, words, FieldPlace::Which);
        match line.which {
            Some(CellWhich::Here) => which(LOOK_AT_HERE),
            Some(CellWhich::Ahead) => which(LOOK_AT_AHEAD),
            Some(CellWhich::At) => {
                let mut parts = which(LOOK_AT_PLACE);
                parts.push(" ");
                parts.append(self.slot_parts(&line.place, None, Blank::Value, "place"));
                parts
            }
            None => which(""),
        }
    }

    /// Returns the parts of an all of, any of or none of line after its name, or of an any of or none of inside a
    /// slot.
    ///
    /// `inline` is whether the phrase sits inside a slot.
    fn list_of(&self, line: &SheetLine, inline: bool) -> Parts {
        let words = match line.kind {
            LineKind::AllOf => "all of",
            LineKind::AnyOf => "any of",
            _ => "none of",
        };
        let mut parts = Parts::lead(inline, words);
        parts.push(" ");
        if line.items.is_empty() {
            parts.append(Parts::placed(
                SentencePart::Blank(Blank::YesOrNoList),
                PlaceTarget::Slot("items.0".to_owned()),
                Some(Blank::YesOrNoList),
            ));
        } else {
            parts.append(Parts::join(
                line.items.iter().enumerate().map(|(index, item)| {
                    self.value_parts(item, None)
                        .under(&format!("items.{index}"), Blank::YesOrNoList)
                }),
                ", ",
            ));
        }
        parts
    }

    /// Returns the parts of a Choose line or phrase after its name.
    ///
    /// `inline` is whether the phrase sits inside a slot.
    fn choose(&self, line: &SheetLine, inline: bool) -> Parts {
        let rows = line.rows.iter().enumerate().map(|(index, row)| {
            let mut parts = self.slot_parts(&row.value, None, Blank::Value, &format!("row.{index}.value"));
            parts.push(", ");
            parts.append(
                self.condition(&row.condition, false)
                    .under(&format!("row.{index}.condition"), Blank::When),
            );
            parts
        });
        let mut otherwise = Parts::lead(inline, "otherwise");
        otherwise.push(" ");
        otherwise.append(self.slot_parts(&line.otherwise, None, Blank::Value, "otherwise"));
        let mut parts = Parts::lead(inline, "choose, first that applies");
        parts.push(": ");
        parts.append(Parts::alternatives(rows.chain(std::iter::once(otherwise)), "; "));
        parts
    }

    /// Returns the parts of a Depending on line or phrase after its name.
    ///
    /// `inline` is whether the phrase sits inside a slot.
    fn depending_on(&self, line: &SheetLine, inline: bool) -> Parts {
        let property = line.property.as_deref();
        let states: Vec<&str> = property
            .and_then(|id| self.properties.get(id))
            .map(|property| property.states.iter().map(|state| state.id.as_str()).collect())
            .unwrap_or_default();
        // Every state of the property takes a branch, a blank one until it holds a value.
        let ordered = states.iter().copied().chain(
            line.branches
                .keys()
                .map(String::as_str)
                .filter(|state| !states.contains(state)),
        );
        let branches = ordered.map(|state| {
            let mut parts = Parts::text(&format!("{} → ", self.state_name(property, Some(state))));
            let key = format!("branches.{state}");
            parts.append(match line.branches.get(state) {
                Some(value) => self.value_parts(value, None).under(&key, Blank::Value),
                None => Parts::placed(
                    SentencePart::Blank(Blank::Value),
                    PlaceTarget::Slot(key),
                    Some(Blank::Value),
                ),
            });
            parts
        });
        let mut parts = Parts::lead(inline, "depending on");
        parts.push(" my ");
        parts.append(self.property_part(property, Blank::Property, FieldPlace::Property));
        parts.push(": ");
        parts.append(Parts::alternatives(branches, ", "));
        parts
    }

    /// Returns the parts of a Calculate line or phrase after its name.
    ///
    /// `inline` is whether the phrase sits inside a slot.
    fn calculate(&self, line: &SheetLine, inline: bool) -> Parts {
        match line.formula.as_deref() {
            Some(formula) if inline => {
                // The card edits the values and the operation of a Calculate line only, never of a formula
                // inside a slot.
                let mut parts = self.formula(formula);
                for placed in &mut parts.0 {
                    placed.place = None;
                }
                parts
            }
            Some(formula) => self.formula(formula),
            None if inline => Parts::part(SentencePart::Blank(Blank::Number)),
            None => {
                // An empty Calculate line offers its two values and its operation.
                let mut parts = Parts::placed(
                    SentencePart::Blank(Blank::Number),
                    field_target(FieldPlace::FirstOperand),
                    Some(Blank::Number),
                );
                parts.push(" ");
                parts.append(Parts::select(LineSelect::Operation, "", FieldPlace::Operation));
                parts.push(" ");
                parts.append(Parts::placed(
                    SentencePart::Blank(Blank::Number),
                    field_target(FieldPlace::SecondOperand),
                    Some(Blank::Number),
                ));
                parts
            }
        }
    }

    /// Returns the parts after "whose" or "only those whose": the value in `slot`, the slot of the key `key`, with the
    /// word "their" of its first chip left out, and its comparison as plain words. An empty `slot` reads as `blank`.
    ///
    /// A comparison there wants a property on its left and, while that is empty or a States property, a state on its
    /// right.
    fn whose(&self, slot: &Slot, blank: Blank, key: &str) -> Parts {
        let mut parts = match slot.value() {
            Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::Compare => {
                self.compare_parts(phrase, true).under(key, blank)
            }
            _ => self.slot_parts(slot, None, blank, key),
        };
        if let Some(PlacedPart {
            part: SentencePart::Chip(chip),
            ..
        }) = parts.0.first_mut()
            && chip.lead == "their"
        {
            chip.lead = "";
        }
        for placed in &mut parts.0 {
            if let SentencePart::Select(LineSelect::Comparison, words) = &mut placed.part {
                placed.part = SentencePart::Text(std::mem::take(words));
                placed.place = None;
            }
        }
        parts.merge_text();
        parts
    }

    /// Returns the parts of a chance after "with chance": its chance and its "for each".
    fn chance_body(&self, line: &SheetLine) -> Parts {
        let mut parts = self.slot_parts(&line.chance, None, Blank::Chance, "chance");
        parts.append(self.for_each(&line.tries, DrawKind::Chance));
        parts
    }

    /// Returns the parts of a rate after "at rate": its rate, its unit and its "for each".
    fn rate_body(&self, line: &SheetLine) -> Parts {
        let mut parts = self.slot_parts(&line.rate, None, Blank::Rate, "rate");
        parts.push(" per ");
        parts.append(Parts::select(
            LineSelect::UnitOfTime,
            line.rate_unit.map(rate_unit_word).unwrap_or_default(),
            FieldPlace::RateUnit,
        ));
        parts.append(self.for_each(&line.tries, DrawKind::Rate));
        parts
    }

    /// Returns " for each ..." for a filled "for each" slot of a draw of `draw`, and the empty optional blank for a
    /// blank one.
    ///
    /// A named result reads in the singular through [`for_each_words`]. The words "for each" and the count are both
    /// placed at the slot `tries`.
    fn for_each(&self, tries: &Slot, draw: DrawKind) -> Parts {
        let slot = || PlaceTarget::Slot("tries".to_owned());
        let value = match tries.value() {
            None => return Parts::placed(SentencePart::ForEachBlank(draw), slot(), Some(Blank::Count)),
            Some(SheetValue::Chip(SheetChip::Result {
                line,
                part: None,
                property: None,
            })) => {
                let name = self
                    .lines
                    .get(line.as_str())
                    .and_then(|line| line.name.as_deref())
                    .map_or_else(|| line.clone(), for_each_words);
                Parts::placed(
                    SentencePart::Chip(SentenceChip::plain(ChipKind::Result(line.clone()), name)),
                    slot(),
                    Some(Blank::Count),
                )
            }
            Some(_) => self.slot_parts(tries, None, Blank::Count, "tries"),
        };
        let mut parts = Parts::text(" ");
        parts.append(Parts::placed(SentencePart::ForEach(draw), slot(), Some(Blank::Count)));
        parts.push(" ");
        parts.append(value);
        parts
    }

    /// Returns the parts of a comparison.
    fn compare(&self, line: &SheetLine) -> Parts {
        self.compare_parts(line, false)
    }

    /// Returns the parts of a comparison, where `whose` is whether it follows "whose": a blank on its left then wants a
    /// property, and a blank on its right a state while the left is empty.
    fn compare_parts(&self, line: &SheetLine, whose: bool) -> Parts {
        let about = line.left.value().and_then(governing_property);
        let of_states = about
            .and_then(|id| self.properties.get(id))
            .is_some_and(|property| property.property_type == PropertyType::States);
        let states_on_right = of_states || (whose && line.left.is_blank());
        let left_blank = if whose { Blank::Property } else { Blank::Value };
        let mut parts = self.slot_parts(&line.left, None, left_blank, "left");
        let comparison = |words: &str| Parts::select(LineSelect::Comparison, words, FieldPlace::Test);
        let Some(test) = line.test else {
            parts.push(" ");
            parts.append(comparison(""));
            parts.push(" ");
            parts.append(self.slot_parts(&line.right, about, Blank::Value, "right"));
            return parts;
        };
        match test {
            CompareTest::Within => {
                parts.push(" is within ");
                parts.append(self.slot_parts(&line.tolerance, None, Blank::Number, "tolerance"));
                parts.push(" of ");
                parts.append(self.slot_parts(&line.right, about, Blank::Number, "right"));
            }
            CompareTest::Outside => {
                parts.push(" is less than ");
                parts.append(self.slot_parts(&line.low, None, Blank::LowerNumber, "low"));
                parts.push(" or more than ");
                parts.append(self.slot_parts(&line.high, None, Blank::UpperNumber, "high"));
            }
            _ => {
                let list = matches!(test, CompareTest::OneOf | CompareTest::NotOneOf);
                let blank = match (states_on_right, list) {
                    (true, true) => Blank::StateList,
                    (true, false) => Blank::State,
                    (false, true) => Blank::NumberList,
                    (false, false) => Blank::Number,
                };
                parts.push(" ");
                parts.append(comparison(comparison_words(test, of_states)));
                parts.push(" ");
                parts.append(self.slot_parts(&line.right, about, blank, "right"));
            }
        }
        parts
    }

    /// Returns the parts of an all of inside a slot: its items joined by "and", and its draws after commas.
    fn inline_all_of(&self, line: &SheetLine) -> Parts {
        let is_draw = |item: &SheetValue| match item {
            SheetValue::Phrase(phrase) => matches!(phrase.kind, LineKind::Chance | LineKind::Rate),
            _ => false,
        };
        let item_parts = |(index, item): (usize, &SheetValue)| {
            self.value_parts(item, None)
                .under(&format!("items.{index}"), Blank::YesOrNo)
        };
        let tests: Vec<Parts> = line
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| !is_draw(item))
            .map(item_parts)
            .collect();
        let mut groups: Vec<Parts> = Vec::new();
        if !tests.is_empty() {
            groups.push(Parts::join(tests, " and "));
        }
        groups.extend(
            line.items
                .iter()
                .enumerate()
                .filter(|(_, item)| is_draw(item))
                .map(item_parts),
        );
        if groups.is_empty() {
            let mut parts = Parts::text("all of ");
            parts.add(SentencePart::Blank(Blank::YesOrNoList));
            parts
        } else {
            Parts::join(groups, ", ")
        }
    }

    /// Returns the parts of the neighbourhood a Count or Look around line looks at.
    fn around(&self, line: &SheetLine) -> Parts {
        match line.around {
            Some(Around::AgentsWithin) => {
                let thing = line
                    .agent_kind
                    .as_deref()
                    .map_or_else(|| BLANK_TEXT.to_owned(), |kind| plural(&self.thing_word(kind)));
                let mut parts = Parts::text(&format!("{thing} within "));
                parts.append(self.slot_parts(&line.distance, None, Blank::Number, "distance"));
                if line.include_me == Some(true) {
                    parts.push(" ");
                    parts.append(Parts::select(
                        LineSelect::IncludeMe,
                        INCLUDING_ME,
                        FieldPlace::IncludeMe,
                    ));
                }
                parts
            }
            around => Parts::select(
                LineSelect::WhichCells,
                around.and_then(around_words).unwrap_or_default(),
                FieldPlace::Around,
            ),
        }
    }

    /// Returns the parts of `formula`: its references by name, its operators as words, and the parentheses of a call
    /// left out.
    ///
    /// The operator of a formula of one operator between two values, as a Calculate line of a cells card holds, is
    /// the select of the operation.
    fn formula(&self, formula: &str) -> Parts {
        let mut words: Vec<Parts> = Vec::new();
        // Whether each open parenthesis is a call's. The words leave a call's parentheses out.
        let mut calls: Vec<bool> = Vec::new();
        let mut glue_next = false;
        let tokens = formula_tokens(formula);
        let binary = tokens.len() == 3
            && tokens
                .get(1)
                .is_some_and(|operator| operation_words(operator).is_some());
        for (index, token) in tokens.iter().enumerate() {
            let previous = index.checked_sub(1).and_then(|before| tokens.get(before));
            let mut word = match token.as_str() {
                "(" => {
                    let call = previous.is_some_and(|previous| function_words(previous).is_some());
                    calls.push(call);
                    if call {
                        continue;
                    }
                    words.push(Parts::text("("));
                    glue_next = true;
                    continue;
                }
                ")" => {
                    if !calls.pop().unwrap_or(false) {
                        match words.last_mut() {
                            Some(last) => last.push(")"),
                            None => words.push(Parts::text(")")),
                        }
                    }
                    continue;
                }
                "," => Parts::text("and"),
                "-" if previous.is_none_or(|previous| {
                    operation_words(previous).is_some() || matches!(previous.as_str(), "(" | ",")
                }) =>
                {
                    words.push(Parts::text("-"));
                    glue_next = true;
                    continue;
                }
                operator if let Some(operation) = operation_words(operator) => {
                    if binary {
                        Parts::part(SentencePart::Select(LineSelect::Operation, operation.to_owned()))
                    } else {
                        Parts::text(operation)
                    }
                }
                function if let Some(words) = function_words(function) => Parts::text(words),
                reference if reference.starts_with(|first: char| first.is_ascii_alphabetic()) => {
                    Parts::part(SentencePart::Chip(self.formula_reference(reference)))
                }
                number if number.starts_with(|first: char| first.is_ascii_digit() || first == '.') => Parts::part(
                    SentencePart::Chip(SentenceChip::plain(ChipKind::Literal, number.to_owned())),
                ),
                other => Parts::text(other),
            };
            if binary {
                let (field, want) = match index {
                    0 => (FieldPlace::FirstOperand, Some(Blank::Number)),
                    1 => (FieldPlace::Operation, None),
                    _ => (FieldPlace::SecondOperand, Some(Blank::Number)),
                };
                for placed in &mut word.0 {
                    if !matches!(placed.part, SentencePart::Text(_)) {
                        placed.place = Some(PartPlace {
                            target: field_target(field),
                            want,
                        });
                    }
                }
            }
            if glue_next {
                match words.last_mut() {
                    Some(last) => last.append(word),
                    None => words.push(word),
                }
                glue_next = false;
            } else {
                words.push(word);
            }
        }
        if words.is_empty() {
            Parts::part(SentencePart::Blank(Blank::Number))
        } else {
            Parts::join(words, " ")
        }
    }

    /// Returns the chip of a reference in a formula: `my.x`, `my_new.x`, `l2`, `l2.amount`, or a parameter id.
    fn formula_reference(&self, reference: &str) -> SentenceChip {
        let (head, tail) = reference.split_once('.').unwrap_or((reference, ""));
        let property = |lead: &'static str| SentenceChip {
            kind: ChipKind::Property(tail.to_owned()),
            lead,
            name: self.property_name(Some(tail)),
        };
        match head {
            "my" => return property("my"),
            "my_new" => return property("my new"),
            _ => {}
        }
        if is_line_id(head) {
            let line = self.lines.get(head);
            let chip = if tail.is_empty() {
                SheetChip::Result {
                    line: head.to_owned(),
                    part: None,
                    property: None,
                }
            } else if matches!(tail, "amount" | "open" | "count")
                || line.is_some_and(|line| line.summaries.iter().any(|summary| summary.id == tail))
            {
                SheetChip::Result {
                    line: head.to_owned(),
                    part: Some(tail.to_owned()),
                    property: None,
                }
            } else {
                SheetChip::Result {
                    line: head.to_owned(),
                    part: None,
                    property: Some(tail.to_owned()),
                }
            };
            return self.chip(&chip, None);
        }
        self.chip(&SheetChip::Param(reference.to_owned()), None)
    }

    /// Returns the name of property `id`, its id when no property has it.
    pub fn property_words(&self, id: &str) -> String {
        self.property_name(Some(id))
    }

    /// Returns the name of property `id`, its id when no property has it, or [`BLANK_TEXT`] for `None`.
    fn property_name(&self, id: Option<&str>) -> String {
        match id {
            None => BLANK_TEXT.to_owned(),
            Some(id) => self
                .properties
                .get(id)
                .map_or_else(|| id.to_owned(), |property| property.name.clone()),
        }
    }

    /// Returns the name of state `id` of property `about`, as [`Self::state_chip`] names it, or [`BLANK_TEXT`] for
    /// `None`.
    fn state_name(&self, about: Option<&str>, id: Option<&str>) -> String {
        id.map_or_else(|| BLANK_TEXT.to_owned(), |id| self.state_chip(about, id).name)
    }

    /// Returns the parts of state `id` of property `about`, the line's field `field`: its chip, or `blank` for `None`.
    fn state_part(&self, about: Option<&str>, id: Option<&str>, blank: Blank, field: FieldPlace) -> Parts {
        Parts::placed(
            id.map_or(SentencePart::Blank(blank), |id| {
                SentencePart::Chip(self.state_chip(about, id))
            }),
            field_target(field),
            Some(blank),
        )
    }

    /// Returns the chip a value of a formula of one operation shows, such as "Infected neighbours" for `l1` or "my
    /// Food" for `my.food`, where `operand` is a reference or a number as the formula writes it.
    pub fn formula_operand(&self, operand: &str) -> SentenceChip {
        if operand.starts_with(|first: char| first.is_ascii_alphabetic()) {
            self.formula_reference(operand)
        } else {
            SentenceChip::plain(ChipKind::Literal, operand.to_owned())
        }
    }

    /// Returns the chip of state `id` of property `about`, or of the first property with such a state when `about`
    /// names none, named as [`shared_state_name`] writes it.
    ///
    /// An unknown state is named by its id and belongs to no property.
    fn state_chip(&self, about: Option<&str>, id: &str) -> SentenceChip {
        let unknown = || {
            SentenceChip::plain(
                ChipKind::State {
                    property: None,
                    state: id.to_owned(),
                },
                id.to_owned(),
            )
        };
        let owner = about
            .and_then(|about| self.properties.get(about))
            .filter(|property| property.states.iter().any(|state| state.id == id))
            .copied()
            .or_else(|| {
                self.all_properties()
                    .find(|property| property.states.iter().any(|state| state.id == id))
            });
        let Some(owner) = owner else {
            return unknown();
        };
        let Some(state) = owner.states.iter().find(|state| state.id == id) else {
            return unknown();
        };
        let shared = self.all_properties().any(|property| {
            property.id != owner.id
                && property.property_type == PropertyType::States
                && property.states.iter().any(|other| other.name == state.name)
        });
        SentenceChip::plain(
            ChipKind::State {
                property: Some(owner.id.clone()),
                state: id.to_owned(),
            },
            shared_state_name(&owner.name, &state.name, shared),
        )
    }

    /// Returns every property of every kind of thing, in sheet order.
    fn all_properties(&self) -> impl Iterator<Item = &'a SheetProperty> {
        let document = self.document;
        document
            .cells
            .iter()
            .flatten()
            .chain(document.agents.iter().flat_map(|kind| &kind.properties))
            .chain(document.ground.iter().flat_map(|ground| &ground.properties))
    }
}

/// Returns whether `id` has the form of a line id, `l` and a number.
fn is_line_id(id: &str) -> bool {
    id.strip_prefix('l')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Returns the words of a literal: a number in the sheet's number form, a percent as typed ("30%"), "yes" or "no".
pub fn literal_words(literal: SheetLiteral) -> String {
    let number = |value: f64| format_number(value).unwrap_or_else(|| value.to_string());
    match literal {
        SheetLiteral::Number(value) => number(value),
        SheetLiteral::Percent(value) => format!("{}%", number(value)),
        SheetLiteral::Bool(true) => "yes".to_owned(),
        SheetLiteral::Bool(false) => "no".to_owned(),
    }
}

/// Returns the select of a Steer or Move line's mode, `select`, showing "by", "towards" or "to", empty for `None`.
fn mode_select(select: LineSelect, mode: Option<LineMode>) -> Parts {
    Parts::select(select, mode.map_or("", LineMode::as_str), FieldPlace::Mode)
}

/// Returns the target of the field `field` of the line or phrase itself, before [`PlaceTarget::under`] places it.
fn field_target(field: FieldPlace) -> PlaceTarget {
    PlaceTarget::Field {
        phrase: String::new(),
        field,
    }
}

/// Returns the two values and the operator of `formula` when it is one operation between two values, as in `l1 *
/// infection_rate`, and `None` for any other formula.
///
/// ```
/// use henad_graph::sheet::sentence::binary_formula;
///
/// assert_eq!(binary_formula("l1*infection_rate"), Some(("l1".to_owned(), '*', "infection_rate".to_owned())));
/// assert_eq!(binary_formula("smaller(l1, 2)"), None);
/// ```
pub fn binary_formula(formula: &str) -> Option<(String, char, String)> {
    let tokens = formula_tokens(formula);
    let [first, operator, second] = tokens.as_slice() else {
        return None;
    };
    let operator = operator
        .chars()
        .next()
        .filter(|_| operation_words(operator).is_some())?;
    let is_value = |token: &str| {
        token.starts_with(|first: char| first.is_ascii_alphanumeric() || first == '.')
            && function_words(token).is_none()
    };
    (is_value(first) && is_value(second)).then(|| (first.clone(), operator, second.clone()))
}

/// Returns the word of a summary of a Look around line, such as "average".
pub(super) fn summary_word(kind: SummaryKind) -> &'static str {
    kind.as_str()
}

/// Returns the tokens of `formula`: numbers, references, operators, parentheses and commas, whitespace left out.
///
/// A number takes its digits, its point and an exponent with its sign, as in `2.5e-7`.
fn formula_tokens(formula: &str) -> Vec<String> {
    let chars: Vec<char> = formula.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while let Some(&c) = chars.get(index) {
        let start = index;
        index += 1;
        if c.is_whitespace() {
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            while let Some(&next) = chars.get(index) {
                let signed_exponent = matches!(next, '+' | '-') && matches!(chars.get(index - 1), Some('e' | 'E'));
                if next.is_ascii_digit() || matches!(next, '.' | 'e' | 'E') || signed_exponent {
                    index += 1;
                } else {
                    break;
                }
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            while chars
                .get(index)
                .is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, '_' | '.'))
            {
                index += 1;
            }
        }
        tokens.push(chars[start..index].iter().collect());
    }
    tokens
}

/// Returns whether `formula` is the length of a property of mine, as `length(my.velocity)`, the one formula a picker
/// puts in a slot.
fn is_length_of_mine(formula: &str) -> bool {
    formula
        .trim()
        .strip_prefix("length(my.")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}
