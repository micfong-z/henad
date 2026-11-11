//! Parts of a line's sentence as the card draws them, and the places an edit of each sets.

use super::card::{ALWAYS, BLANK_TEXT, Blank, DrawKind, FROM_WORD, LineSelect};
use super::notes::otherwise_words;
use super::rates::COMPETING_WORD;

// Parts of a sentence

/// One part of a line's sentence as the card draws it.
///
/// [`Sentences::line_parts`] returns a line's parts in order, and [`sentence_text`] joins them into the sentence as
/// plain text.
///
/// [`Sentences::line_parts`]: super::Sentences::line_parts
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SentencePart {
    /// Words between the values of the line, such as " whose ".
    Text(String),
    /// A lead word that says what the line does, drawn in bold: the verb of a change ("Become", "Set", "Steer",
    /// "Keep", "Move", "Leave", "Record"), "When" and "Repeat" before a blank or "always", and the words that name
    /// what a result line works out ("count of", "look at", "all of", "choose, first that applies", "depending on")
    /// with the "otherwise" of a Choose.
    Keyword(String),
    /// The name of the result a result line works out, before " = ", drawn as the line's result chip.
    ResultName(String),
    /// The start of one alternative of a Choose or a Depending on line, which the card draws on a row of its own.
    /// Plain text reads it as nothing.
    Alternative,
    /// A condition word in plain bold text: "when", "with chance" or "at rate", or on a When line "When", "When with
    /// chance" or "When at rate".
    ConditionWord(&'static str),
    /// The word "from" of a Become line, which has the tooltip of [`from_tooltip`](super::from_tooltip).
    From,
    /// A chosen value, drawn as a chip.
    Chip(SentenceChip),
    /// An empty slot, drawn as a dashed button that shows [`Blank::word`]. Plain text reads it as [`BLANK_TEXT`].
    Blank(Blank),
    /// A select inside the line and the words chosen in it, empty while nothing is chosen. Plain text reads an empty
    /// select as [`BLANK_TEXT`].
    Select(LineSelect, String),
    /// The grey "always" of a change whose condition always holds.
    Always,
    /// The words "otherwise (when line 2 does not apply)" before a change, with the numbers of the higher lines that
    /// override it.
    Otherwise(Vec<usize>),
    /// The word "or" before a Become of a competing group below its highest line, with the numbers of the group's
    /// lines. [`competing_parts`](super::competing_parts) puts it in place.
    Competing(Vec<usize>),
    /// The empty optional "for each ..." blank after a chance or a rate. Plain text leaves it out.
    ForEachBlank(DrawKind),
    /// The words "for each" of a chance or a rate whose "for each" is on, before its count.
    ForEach(DrawKind),
}

impl SentencePart {
    /// Returns the part as plain text.
    pub fn text(&self) -> String {
        match self {
            Self::Select(_, words) if words.is_empty() => BLANK_TEXT.to_owned(),
            Self::Text(words) | Self::Keyword(words) | Self::ResultName(words) | Self::Select(_, words) => {
                words.clone()
            }
            Self::ConditionWord(words) => (*words).to_owned(),
            Self::From => FROM_WORD.to_owned(),
            Self::Chip(chip) => chip.text(),
            Self::Blank(_) => BLANK_TEXT.to_owned(),
            Self::Always => ALWAYS.to_owned(),
            Self::Otherwise(lines) => otherwise_words(lines),
            Self::Competing(_) => COMPETING_WORD.to_owned(),
            Self::ForEachBlank(_) | Self::Alternative => String::new(),
            Self::ForEach(_) => FOR_EACH_WORDS.to_owned(),
        }
    }
}

/// Words of a chance or a rate whose "for each" is on, before its count.
pub const FOR_EACH_WORDS: &str = "for each";

/// One part of a line's sentence with the place in the line that an edit of it sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedPart {
    /// The part.
    pub part: SentencePart,
    /// Place of the part in the line, `None` for words and for a part that no edit sets.
    pub place: Option<PartPlace>,
}

/// Place of a chip, a blank or a select of a line's sentence: the slot or the field that holds what it shows, and what
/// the place wants.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PartPlace {
    /// Slot or field of the line, or of a phrase inside it.
    pub target: PlaceTarget,
    /// Kind of value the place takes, the blank it shows while empty, `None` for a select.
    pub want: Option<Blank>,
}

/// A slot or a field of a line, or of a phrase inside it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlaceTarget {
    /// A slot, by its slot path, such as `condition.right`.
    Slot(String),
    /// An item of the list a slot holds, by the slot's path and the item's index from 0.
    ListItem {
        /// Slot path of the slot that holds the list.
        slot: String,
        /// Index of the item in the list.
        index: usize,
    },
    /// A field of the line, or of the phrase at a slot path.
    Field {
        /// Slot path of the phrase that holds the field, empty for the line itself.
        phrase: String,
        /// The field.
        field: FieldPlace,
    },
}

/// A field of a line or a phrase that a part of its sentence shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldPlace {
    /// The property a Set line changes.
    Property,
    /// The new state of a Become line.
    NewState,
    /// The old state of a Become line.
    StartState,
    /// The test of a comparison.
    Test,
    /// The neighbourhood of a Count or a Total or average line.
    Around,
    /// The unit of a rate.
    RateUnit,
    /// Total or average, the summary of a Total or average line.
    Summary,
    /// The value of a Total or average line.
    SummaryValue,
    /// The operation of a Calculate line of one operation.
    Operation,
    /// The value left of the operation of a Calculate line of one operation.
    FirstOperand,
    /// The value right of the operation of a Calculate line of one operation.
    SecondOperand,
    /// The mode of a Steer or Move line.
    Mode,
    /// Rule a Pick a cell line picks by.
    Pick,
    /// The ground property a Pick a cell line ranks cells by.
    RankedBy,
    /// The cell a Look at a cell line looks at.
    Which,
    /// The ground property a Leave line marks.
    GroundProperty,
    /// The event a Record line records.
    Event,
    /// Kind of the summary at this index of a look at agents, the index counted from 0.
    LookSummary(usize),
}

impl PlaceTarget {
    /// Returns the target of `self`, a target relative to the slot `key`, as a target of the line.
    pub(super) fn under(self, key: &str) -> Self {
        let join = |path: String| match (key.is_empty(), path.is_empty()) {
            (true, _) => path,
            (false, true) => key.to_owned(),
            (false, false) => format!("{key}.{path}"),
        };
        match self {
            Self::Slot(path) => Self::Slot(join(path)),
            Self::ListItem { slot, index } => Self::ListItem {
                slot: join(slot),
                index,
            },
            Self::Field { phrase, field } => Self::Field {
                phrase: join(phrase),
                field,
            },
        }
    }

    /// Returns whether the target is the slot itself or an item of its list, as a value that a slot holds directly
    /// places its parts before [`Self::under`].
    pub(super) fn is_direct(&self) -> bool {
        match self {
            Self::Slot(path) | Self::ListItem { slot: path, .. } => path.is_empty(),
            Self::Field { .. } => false,
        }
    }
}

/// A chosen value in a line's sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentenceChip {
    /// Kind of thing the chip refers to. It picks the chip's colour.
    pub kind: ChipKind,
    /// Grey word before the name: "my", "my new" or "their", empty for any other chip.
    pub lead: &'static str,
    /// Name of what the chip refers to, such as "Alive neighbours", "Status Alive" or "3".
    pub name: String,
}

impl SentenceChip {
    /// Returns a chip of `kind` named `name`, with no lead word.
    pub(super) fn plain(kind: ChipKind, name: String) -> Self {
        Self { kind, lead: "", name }
    }

    /// Returns the visible text of the chip: its lead and its name, such as "my Food".
    pub fn text(&self) -> String {
        if self.lead.is_empty() {
            self.name.clone()
        } else {
            format!("{} {}", self.lead, self.name)
        }
    }
}

/// Kind of thing a chip of a sentence refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChipKind {
    /// A typed number, a percent, yes or no.
    Literal,
    /// The parameter of this id.
    Parameter(String),
    /// The property of this id, of the thing itself or of the thing it looks at.
    Property(String),
    /// A state of a property.
    State {
        /// Id of the property the state belongs to, `None` for a state that no property has.
        property: Option<String>,
        /// Id of the state.
        state: String,
    },
    /// Any state, the old state of a Become line that changes a cell in any state.
    AnyState,
    /// The result of the line of this id, or a part or a property of it.
    Result(String),
    /// A relation to the agent looked at, such as "distance to them".
    Relation,
    /// The event of this id.
    Event(String),
}

/// Returns `parts` joined into the sentence as plain text.
pub fn sentence_text(parts: &[SentencePart]) -> String {
    parts.iter().map(SentencePart::text).collect()
}
