//! Words of the sheet editor: the wording table, and the sentence of each line written from it.
//!
//! The *wording table* is every constant and function of this module that returns text the sheet editor shows. On the
//! card: its frame, the line sentences and their parts, the comparisons, the conditions and "Choose when", blanks and
//! their accessible names, pickers, add menus, result names, the notes under a line, "otherwise", the words and notes
//! of rates and chances, the Lines help and the card's messages. Around the card: the sheet and its forms, the New
//! model and New parameter dialogs, the top bar, the world tabs, the help popovers, the Diagram and the conversion to a
//! node graph. The comments of the sheet writer and every editor take their words from here.
//!
//! Terms the table uses:
//!
//! - A *line number* is a line's place on its card, counted from 1 in display order, the lines inside a When or
//!   Repeat counted after it. It differs from the number in the line's id.
//! - A *line word* names a line inside a sentence: "line 2", or on a button's card "Clear line 1".
//! - A *line prefix* starts an accessible name: "Line 2", or on a button's card "Clear line 1".
//! - A *blank* is an empty slot, written `...` in a sentence written as plain text.
//!
//! [`Sentences`] writes the sentence of a line and the heading of a card from a [`SheetDocument`].

use std::collections::HashMap;
use std::fmt;

use henad_core::metadata::TimeUnit;

use super::document::{
    Around, CardRuns, CellWhich, ChipRelation, CompareTest, LineKind, LineMode, PickRule, PropertyType, SheetCard,
    SheetChip, SheetDocument, SheetEvent, SheetLine, SheetLiteral, SheetParameter, SheetProperty, SheetValue, Slot,
    SummaryKind,
};
use super::writer::format_number;

/// Text of a blank in a sentence written as plain text.
pub const BLANK_TEXT: &str = "...";

/// Reserved state id of a Become line that changes a cell in any state.
pub const ANY_STATE_ID: &str = "any_state";

// The card's frame

/// Returns the header of a card that runs every tick over `thing`, such as "Each tick, every cell".
pub fn card_header(thing: &str) -> String {
    format!("Each tick, every {thing}")
}

/// Returns the number of lines a card holds, as its header shows it: "1 line", "3 lines".
pub fn line_count(count: usize) -> String {
    if count == 1 {
        "1 line".to_owned()
    } else {
        format!("{count} lines")
    }
}

/// Law of the card, shown in a box under its header.
pub const LAW: &str = "Everyone reads the world as it was at the start of the tick. All changes land together at the \
                       end of it. Whatever no line changes stays as it was.";

/// Label of the add button of agent cards that adds a Repeat line.
pub const REPEAT_BUTTON: &str = "+ Repeat...";

/// Accessible name of the "?" beside the add buttons, which opens the Lines help.
pub const HELP_LINES_NAME: &str = "Help: lines";

/// Note above the add buttons once the card holds a When line.
pub const ADD_AT_END_NOTE: &str = "These add a line at the end of the card.";

/// Returns the label of the row that ends When line `number`, such as "+ Add inside line 4".
pub fn add_inside_label(number: usize) -> String {
    format!("+ Add inside line {number}")
}

/// Returns the accessible name of the row that ends When line `number`, which is also the title of its menu, such as
/// "Add a line inside line 4".
pub fn add_inside_name(number: usize) -> String {
    format!("Add a line inside line {number}")
}

/// Returns the line word of line `number`: "line 2", or "Clear line 1" on the card of the button `button`.
pub fn line_word(button: Option<&str>, number: usize) -> String {
    match button {
        Some(button) => format!("{button} line {number}"),
        None => format!("line {number}"),
    }
}

/// Returns the line prefix of line `number`: "Line 2", or "Clear line 1" on the card of the button `button`.
pub fn line_prefix(button: Option<&str>, number: usize) -> String {
    match button {
        Some(button) => format!("{button} line {number}"),
        None => format!("Line {number}"),
    }
}

/// Returns the name of the menu of a line's tools, such as "Line 2 actions", where `prefix` is the line prefix.
pub fn line_actions_name(prefix: &str) -> String {
    format!("{prefix} actions")
}

/// Returns the tool that renames a result line, such as "Rename line 2", where `word` is the line word.
pub fn rename_line(word: &str) -> String {
    format!("Rename {word}")
}

/// Returns the tool that moves a line up, such as "Move line 2 up", where `word` is the line word.
pub fn move_line_up(word: &str) -> String {
    format!("Move {word} up")
}

/// Returns the tool that moves a line down, such as "Move line 2 down", where `word` is the line word.
pub fn move_line_down(word: &str) -> String {
    format!("Move {word} down")
}

/// Returns the tool that deletes a line, such as "Delete line 2", where `word` is the line word.
pub fn delete_line(word: &str) -> String {
    format!("Delete {word}")
}

/// Tooltip of the tool that deletes a When line.
pub const DELETE_WHEN_TOOLTIP: &str = "Delete. Lines inside stay.";

/// Item of a line's menu that mints a new key for its draws.
pub const DRAW_NEW_NUMBERS: &str = "Draw new numbers";

/// Returns the message for a rename to `name`, which another result already holds.
pub fn result_name_taken(name: &str) -> String {
    format!("Another result is already called {name}.")
}

/// Returns the accessible name of the field that renames a line, such as "Line 2: new name".
pub fn rename_field_name(prefix: &str) -> String {
    format!("{prefix}: new name")
}

/// Returns the heading of the card of the button `button` over `thing`, such as "Lines of Clear: run once, every
/// cell, when pressed".
pub fn button_card_heading(button: &str, thing: &str) -> String {
    format!("Lines of {button}: run once, every {thing}, when pressed")
}

/// Text of a button's card that holds no line.
pub const BUTTON_CARD_EMPTY: &str = "No lines yet. Add a change below.";

/// Returns the accessible name of a card's list of lines: "Lines", or "Lines of Clear" for the card of a button.
pub fn line_list_name(button: Option<&str>) -> String {
    match button {
        Some(button) => format!("Lines of {button}"),
        None => "Lines".to_owned(),
    }
}

/// Text of the card of a model that holds no line.
pub const BLANK_MODEL_CARD: &str = "Nothing changes yet. Add a line below.";

/// One add button under a card's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddButton {
    /// "+ Look around".
    LookAround,
    /// "+ Chance".
    Chance,
    /// "+ Yes or no".
    YesOrNo,
    /// "+ Calculate".
    Calculate,
    /// "+ Change".
    Change,
    /// "+ When, for several lines".
    When,
}

impl AddButton {
    /// Every add button, in the order the card shows them.
    pub const ALL: [Self; 6] = [
        Self::LookAround,
        Self::Chance,
        Self::YesOrNo,
        Self::Calculate,
        Self::Change,
        Self::When,
    ];

    /// Groups of the menu of a When block's add row, in order.
    pub const ADD_INSIDE_ORDER: [Self; 6] = [
        Self::Change,
        Self::When,
        Self::Chance,
        Self::LookAround,
        Self::YesOrNo,
        Self::Calculate,
    ];

    /// Label of the button.
    pub fn label(self) -> &'static str {
        match self {
            Self::LookAround => "+ Look around",
            Self::Chance => "+ Chance",
            Self::YesOrNo => "+ Yes or no",
            Self::Calculate => "+ Calculate",
            Self::Change => "+ Change",
            Self::When => "+ When, for several lines",
        }
    }

    /// Tooltip of the button.
    pub fn tooltip(self) -> &'static str {
        match self {
            Self::LookAround => "Count or total of the cells around me (neighbours)",
            Self::Chance => "Change a state by chance",
            Self::YesOrNo => "Compare values, or join yes or no results",
            Self::Calculate => "Work out a number from two numbers",
            Self::Change => "Change a state or a number",
            Self::When => "Give several lines one shared condition",
        }
    }

    /// Heading of the button's menu.
    pub fn heading(self) -> &'static str {
        match self {
            Self::LookAround => "Look at the cells around me (neighbours)",
            Self::Chance => "Change a state by chance",
            Self::YesOrNo => "Make a yes or no result",
            Self::Calculate => "Work out a number",
            Self::Change => "Change one of my properties",
            Self::When => "Several lines that share one condition",
        }
    }

    /// Items of the button's menu, in order.
    pub fn items(self) -> &'static [AddItem] {
        match self {
            Self::LookAround => &[AddItem::Count, AddItem::TotalOrAverage],
            Self::Chance => &[
                AddItem::BecomeWithChance,
                AddItem::BecomeAtRate,
                AddItem::WithChance,
                AddItem::AtRate,
            ],
            Self::YesOrNo => &[AddItem::Compare, AddItem::AllOf, AddItem::AnyOf],
            Self::Calculate => &[AddItem::Calculate],
            Self::Change => &[AddItem::Become, AddItem::Set],
            Self::When => &[AddItem::When],
        }
    }
}

/// Heading of the group of the Chance menu that adds a yes or no result, above [`AddItem::WithChance`] and
/// [`AddItem::AtRate`].
pub const SHARED_DRAW_GROUP: &str = "Or a yes or no that several lines share";

/// One item of an add menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddItem {
    /// Count, under "+ Look around".
    Count,
    /// Total or average, under "+ Look around".
    TotalOrAverage,
    /// Become, with chance, under "+ Chance".
    BecomeWithChance,
    /// Become, at a rate, under "+ Chance".
    BecomeAtRate,
    /// With chance, a yes or no result under "+ Chance".
    WithChance,
    /// At a rate, a yes or no result under "+ Chance".
    AtRate,
    /// Compare, under "+ Yes or no".
    Compare,
    /// All of, under "+ Yes or no".
    AllOf,
    /// Any of, under "+ Yes or no".
    AnyOf,
    /// Calculate, under "+ Calculate".
    Calculate,
    /// Become, under "+ Change".
    Become,
    /// Set, under "+ Change".
    Set,
    /// When, under "+ When, for several lines".
    When,
}

impl AddItem {
    /// Label of the item, which is also its accessible name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Count => "Count",
            Self::TotalOrAverage => "Total or average",
            Self::BecomeWithChance => "Become, with chance",
            Self::BecomeAtRate => "Become, at a rate",
            Self::WithChance => "With chance",
            Self::AtRate => "At a rate",
            Self::Compare => "Compare",
            Self::AllOf => "All of",
            Self::AnyOf => "Any of",
            Self::Calculate => "Calculate",
            Self::Become => "Become",
            Self::Set => "Set",
            Self::When => "When",
        }
    }

    /// Returns the grey sentence pattern under the item.
    ///
    /// `states` holds the second and first states of the model's first States property with two or more states, as
    /// `(new, old)`. The three Become patterns name them, or "a new state" and "an old state" when `states` is `None`.
    pub fn pattern(self, states: Option<(&str, &str)>) -> String {
        let (new, old) = states.unwrap_or(("a new state", "an old state"));
        match self {
            Self::Count => "count of the cells around me (8, 4 or a 3 by 3 block) whose ... is ...".to_owned(),
            Self::TotalOrAverage => "total of ... over the cells around me (8, 4 or a 3 by 3 block)".to_owned(),
            Self::BecomeWithChance => format!("Become {new} from {old}, with chance ..., optionally for each ..."),
            Self::BecomeAtRate => format!("Become {new} from {old}, at rate ... per day, optionally for each ..."),
            Self::WithChance => "a yes or no: with chance ...".to_owned(),
            Self::AtRate => "a yes or no: at rate ... per day".to_owned(),
            Self::Compare => "... is ..., is at least ..., is more than ... and others".to_owned(),
            Self::AllOf => "all of ..., ...".to_owned(),
            Self::AnyOf => "any of ..., ...".to_owned(),
            Self::Calculate => "... plus ...".to_owned(),
            Self::Become => format!("Change a state: Become {new} from {old}, when ..."),
            Self::Set => "Change a number: Set ... to ..., when ...".to_owned(),
            Self::When => "When ...: then add the lines that share it".to_owned(),
        }
    }
}

// Line sentences

/// Returns a unit of time as a rate line reads it, in the singular: "day".
pub fn rate_unit_word(unit: TimeUnit) -> &'static str {
    unit.as_str()
}

/// Returns the words of a neighbourhood of cells: "the 8 cells around me", "the 4 cells around me" or "the 3 by 3
/// block including me".
///
/// [`Around::AgentsWithin`] returns `None`. Its words name a kind of agent and a distance, and [`Sentences`] writes
/// them.
pub fn around_words(around: Around) -> Option<&'static str> {
    match around {
        Around::Cells8 => Some("the 8 cells around me"),
        Around::Cells4 => Some("the 4 cells around me"),
        Around::Block3x3 => Some("the 3 by 3 block including me"),
        Around::AgentsWithin => None,
    }
}

/// Returns the operation word of a Calculate line: "plus", "minus", "times" or "divided by", for `+`, `-`, `*` and `/`.
///
/// `^` reads "to the power of". Any other text returns `None`.
pub fn operation_words(operator: &str) -> Option<&'static str> {
    match operator {
        "+" => Some("plus"),
        "-" => Some("minus"),
        "*" => Some("times"),
        "/" => Some("divided by"),
        "^" => Some("to the power of"),
        _ => None,
    }
}

/// Returns the words of a function of a formula: "length of", "direction of", "smaller of" or "larger of".
pub fn function_words(function: &str) -> Option<&'static str> {
    match function {
        "length" => Some("length of"),
        "direction" => Some("direction of"),
        "smaller" => Some("smaller of"),
        "larger" => Some("larger of"),
        _ => None,
    }
}

/// Returns the words of a relation chip: "offset to them", "offset away from them" or "distance to them".
pub fn relation_words(relation: ChipRelation) -> &'static str {
    match relation {
        ChipRelation::OffsetToThem => "offset to them",
        ChipRelation::OffsetAwayFromThem => "offset away from them",
        ChipRelation::DistanceToThem => "distance to them",
    }
}

// Comparisons

/// Comparisons of the comparison select, in the order it offers them.
pub const COMPARISONS: [CompareTest; 8] = [
    CompareTest::Is,
    CompareTest::IsNot,
    CompareTest::AtLeast,
    CompareTest::AtMost,
    CompareTest::MoreThan,
    CompareTest::LessThan,
    CompareTest::OneOf,
    CompareTest::NotOneOf,
];

/// Returns the words of a comparison, where `of_states` is whether the value on the left is a States property.
///
/// With a States property on the left, [`CompareTest::OneOf`] and [`CompareTest::NotOneOf`] read "is one of the
/// states" and "is none of the states". [`CompareTest::Within`] reads "is within" before its tolerance, and
/// [`CompareTest::Outside`] reads "is less than ... or more than ...", whose numbers [`outside_words`] fills in.
pub fn comparison_words(test: CompareTest, of_states: bool) -> &'static str {
    match test {
        CompareTest::Is => "is",
        CompareTest::IsNot => "is not",
        CompareTest::AtLeast => "is at least",
        CompareTest::AtMost => "is at most",
        CompareTest::MoreThan => "is more than",
        CompareTest::LessThan => "is less than",
        CompareTest::OneOf if of_states => "is one of the states",
        CompareTest::OneOf => "is one of the numbers",
        CompareTest::NotOneOf if of_states => "is none of the states",
        CompareTest::NotOneOf => "is none of the numbers",
        CompareTest::Within => "is within",
        CompareTest::Outside => "is less than ... or more than ...",
    }
}

/// Returns the outside test of `value` against `low` and `high`, such as "Alive neighbours is less than 2 or more than
/// 3".
pub fn outside_words(value: &str, low: &str, high: &str) -> String {
    format!("{value} is less than {low} or more than {high}")
}

// Conditions and "Choose when"

/// The condition that always holds, a grey button after the comma of a change.
pub const ALWAYS: &str = "always";

/// Returns the accessible name of the "always" button, such as "Line 2: choose when, now always".
pub fn always_name(prefix: &str) -> String {
    format!("{prefix}: choose when, now always")
}

/// Tooltip of the "always" button.
pub const ALWAYS_TOOLTIP: &str = "No condition. Choose to add one.";

/// Kind of a condition that is not always: a test, a chance or a rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConditionKind {
    /// A yes or no value: a result, a comparison or an outside test.
    When,
    /// A chance.
    Chance,
    /// A rate.
    Rate,
}

/// Returns the condition word of `kind`, in plain bold text before the condition: "when", "with chance" or "at rate",
/// or on a When line "When", "When with chance" or "When at rate".
pub fn condition_head(kind: ConditionKind, on_when_line: bool) -> &'static str {
    match (kind, on_when_line) {
        (ConditionKind::When, false) => "when",
        (ConditionKind::Chance, false) => "with chance",
        (ConditionKind::Rate, false) => "at rate",
        (ConditionKind::When, true) => "When",
        (ConditionKind::Chance, true) => "When with chance",
        (ConditionKind::Rate, true) => "When at rate",
    }
}

/// Returns the accessible name of the arrow after a condition word, such as "Line 2: change the kind of condition".
pub fn condition_arrow_name(prefix: &str) -> String {
    format!("{prefix}: change the kind of condition")
}

/// Tooltip of the arrow after a condition word.
pub const CONDITION_ARROW_TOOLTIP: &str = "Change the kind of condition";

/// Title of the picker of a condition.
pub const CHOOSE_WHEN: &str = "Choose when";

/// Returns the lead of "Choose when" inside When line `number`.
pub fn when_lead(number: usize) -> String {
    format!("Line {number} already gives a condition. Choose always to use only that one.")
}

/// Heading of the group of results above a line, in "Choose when" and in a picker of a number.
pub const FROM_LINES_ABOVE: &str = "From lines above";

/// Returns the item of "Choose when" that uses the yes or no result `name`: "when Crowded", or "When Crowded" on a When
/// line.
pub fn result_condition_item(name: &str, on_when_line: bool) -> String {
    let head = condition_head(ConditionKind::When, on_when_line);
    format!("{head} {name}")
}

/// Returns the grey line under a result in a picker, such as "line 1: count of the 8 cells around me whose Status is
/// Alive", where `sentence` is the result line's sentence after its name.
pub fn result_description(number: usize, sentence: &str) -> String {
    format!("line {number}: {sentence}")
}

/// Heading of the group of condition forms when no result is listed above it.
pub const CONDITIONS: &str = "Conditions";

/// Heading of the group of condition forms below the results.
pub const OR_A_NEW_CONDITION: &str = "Or a new condition";

/// The item of "Choose when" in place of "always" below a change it can collide with, stored as always.
pub const OTHERWISE: &str = "otherwise";

/// Returns the grey line under the "otherwise" item, such as "when line 2 does not apply" or "when lines 2 and 5 do
/// not apply", where `lines` holds the numbers of the higher lines.
pub fn otherwise_description(lines: &[usize]) -> String {
    if lines.len() == 1 {
        format!("when line {} does not apply", number_list(lines))
    } else {
        format!("when lines {} do not apply", number_list(lines))
    }
}

/// Returns the accessible name of the "otherwise" item, such as "otherwise, when line 2 does not apply".
pub fn otherwise_item_name(lines: &[usize]) -> String {
    format!("{OTHERWISE}, {}", otherwise_description(lines))
}

/// One form of a condition that "Choose when" offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConditionItem {
    /// "always".
    Always,
    /// "when ... is ...".
    Is,
    /// "when ... is at least ...".
    AtLeast,
    /// "when ... is at most ...".
    AtMost,
    /// "when ... is less than ... or more than ...".
    Outside,
    /// "with chance ...".
    WithChance,
    /// "at rate ... per ...".
    AtRate,
}

impl ConditionItem {
    /// Every form, in the order "Choose when" lists them.
    pub const ALL: [Self; 7] = [
        Self::Always,
        Self::Is,
        Self::AtLeast,
        Self::AtMost,
        Self::Outside,
        Self::WithChance,
        Self::AtRate,
    ];

    /// Returns the label of the item, which starts with "When" on a When line.
    pub fn label(self, on_when_line: bool) -> String {
        let when = condition_head(ConditionKind::When, on_when_line);
        match self {
            Self::Always => ALWAYS.to_owned(),
            Self::Is => format!("{when} ... is ..."),
            Self::AtLeast => format!("{when} ... is at least ..."),
            Self::AtMost => format!("{when} ... is at most ..."),
            Self::Outside => format!("{when} ... is less than ... or more than ..."),
            Self::WithChance => format!("{} ...", condition_head(ConditionKind::Chance, on_when_line)),
            Self::AtRate => format!("{} ... per ...", condition_head(ConditionKind::Rate, on_when_line)),
        }
    }

    /// Grey line under the item, `None` for an item without one.
    pub fn description(self) -> Option<&'static str> {
        match self {
            Self::Is => Some("The same as a value. The line offers is not, is more than and others."),
            Self::AtLeast => Some("The value or more, such as 1 or more"),
            Self::AtMost => Some("The value or less, such as 4 or less"),
            Self::Outside => Some("Outside a range, such as less than 2 or more than 3"),
            Self::Always | Self::WithChance | Self::AtRate => None,
        }
    }
}

// Blanks and their accessible names

/// What a blank wants. It names the blank, titles its picker and words its skip note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Blank {
    /// A state.
    State,
    /// The state a Become changes from.
    OldState,
    /// A property.
    Property,
    /// A Number property.
    NumberProperty,
    /// The value on the left of a comparison.
    Value,
    /// A number.
    Number,
    /// A list of numbers.
    NumberList,
    /// A list of states.
    StateList,
    /// A chance.
    Chance,
    /// A rate.
    Rate,
    /// The count after "for each", or the count of a Count line.
    Count,
    /// A yes or no result.
    YesOrNo,
    /// Yes or no results.
    YesOrNoList,
    /// The lower number of an outside test.
    LowerNumber,
    /// The upper number of an outside test.
    UpperNumber,
    /// A condition.
    When,
}

impl Blank {
    /// Every blank.
    pub const ALL: [Self; 16] = [
        Self::State,
        Self::OldState,
        Self::Property,
        Self::NumberProperty,
        Self::Value,
        Self::Number,
        Self::NumberList,
        Self::StateList,
        Self::Chance,
        Self::Rate,
        Self::Count,
        Self::YesOrNo,
        Self::YesOrNoList,
        Self::LowerNumber,
        Self::UpperNumber,
        Self::When,
    ];

    /// Visible word of the blank, drawn in its dashed button.
    pub fn word(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::OldState => "old state",
            Self::Property => "property",
            Self::NumberProperty => "number property",
            Self::Value => "value",
            Self::Number | Self::LowerNumber | Self::UpperNumber => "number",
            Self::NumberList => "list of numbers",
            Self::StateList => "list of states",
            Self::Chance => "chance",
            Self::Rate => "rate",
            Self::Count => "count",
            Self::YesOrNo => "result",
            Self::YesOrNoList => "results",
            Self::When => "when",
        }
    }

    /// Words of what the blank wants, after "choose": "a state", "the old state", "what to count".
    pub fn want(self) -> &'static str {
        match self {
            Self::State => "a state",
            Self::OldState => "the old state",
            Self::Property => "a property",
            Self::NumberProperty => "a number property",
            Self::Value => "a value",
            Self::Number => "a number",
            Self::NumberList => "a list of numbers",
            Self::StateList => "a list of states",
            Self::Chance => "a chance",
            Self::Rate => "a rate",
            Self::Count => "what to count",
            Self::YesOrNo => "a yes or no result",
            Self::YesOrNoList => "yes or no results",
            Self::LowerNumber => "the lower number",
            Self::UpperNumber => "the upper number",
            Self::When => "when",
        }
    }

    /// Returns the accessible name of the blank, such as "Line 2: choose a state".
    pub fn name(self, prefix: &str) -> String {
        format!("{prefix}: choose {}", self.want())
    }

    /// Returns the title of the blank's picker, such as "Choose a state".
    pub fn picker_title(self) -> String {
        format!("Choose {}", self.want())
    }
}

/// One select inside a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LineSelect {
    /// The unit of a rate.
    UnitOfTime,
    /// The comparison of a Compare line or condition.
    Comparison,
    /// The neighbourhood of a Count or Total or average line.
    WhichCells,
    /// Total or average.
    TotalOrAverage,
    /// The operation of a Calculate line.
    Operation,
}

impl LineSelect {
    /// Every select.
    pub const ALL: [Self; 5] = [
        Self::UnitOfTime,
        Self::Comparison,
        Self::WhichCells,
        Self::TotalOrAverage,
        Self::Operation,
    ];

    /// Words of the select, after the line prefix in its accessible name.
    pub fn words(self) -> &'static str {
        match self {
            Self::UnitOfTime => "unit of time",
            Self::Comparison => "comparison",
            Self::WhichCells => "which cells",
            Self::TotalOrAverage => "total or average",
            Self::Operation => "operation",
        }
    }

    /// Returns the accessible name of the select, such as "Line 2: unit of time".
    pub fn name(self, prefix: &str) -> String {
        format!("{prefix}: {}", self.words())
    }
}

/// Grey word a select inside a line shows while it is empty.
pub const UNIT_PLACEHOLDER: &str = "unit";

/// A draw a line makes, which the words of "for each" and its notes name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DrawKind {
    /// A chance.
    Chance,
    /// A rate.
    Rate,
}

impl DrawKind {
    /// Word of the draw: "chance" or "rate".
    pub fn word(self) -> &'static str {
        match self {
            Self::Chance => "chance",
            Self::Rate => "rate",
        }
    }
}

/// Returns the accessible name of the optional "for each ..." blank, such as "Line 2: for each, optional".
pub fn for_each_optional_name(prefix: &str) -> String {
    format!("{prefix}: for each, optional")
}

/// Returns the tooltip of the optional "for each ..." blank: "Optional: make the rate grow with a count", or the same
/// with "chance".
pub fn for_each_optional_tooltip(draw: DrawKind) -> String {
    format!("Optional: make the {} grow with a count", draw.word())
}

/// Returns the accessible name of the words "for each" once they are on, such as "Line 2: change or remove for each".
pub fn for_each_change_name(prefix: &str) -> String {
    format!("{prefix}: change or remove for each")
}

/// Returns the accessible name of the "+" after a list, such as "Line 3: add another value".
pub fn add_value_name(prefix: &str) -> String {
    format!("{prefix}: add another value")
}

/// Returns the label of the button of the "for each" prompt, such as "Add for each infected neighbour", where `count`
/// is the count's words after "for each" from [`for_each_words`].
pub fn add_for_each_label(count: &str) -> String {
    format!("Add for each {count}")
}

/// Returns the accessible name of the button of the "for each" prompt, such as "Line 2: add for each infected
/// neighbour".
pub fn add_for_each_name(prefix: &str, count: &str) -> String {
    format!("{prefix}: add for each {count}")
}

/// Returns the label of the button that moves a count into "for each", such as "Use a rate for each infected
/// neighbour".
pub fn use_for_each_label(draw: DrawKind, count: &str) -> String {
    format!("Use a {} for each {count}", draw.word())
}

/// Returns the accessible name of the button that moves a count into "for each", such as "Line 2: use a rate for each
/// infected neighbour".
pub fn use_for_each_name(prefix: &str, draw: DrawKind, count: &str) -> String {
    format!("{prefix}: use a {} for each {count}", draw.word())
}

/// Returns the label of the button that replaces a one-sided outside test, such as "Use is more than 0 instead", where
/// `comparison` is the comparison with its number.
pub fn use_instead_label(comparison: &str) -> String {
    format!("Use {comparison} instead")
}

/// Returns the accessible name of the button that replaces a one-sided outside test, such as "Line 5: use is more
/// than 0 instead".
pub fn use_instead_name(prefix: &str, comparison: &str) -> String {
    format!("{prefix}: use {comparison} instead")
}

/// Tooltip of the word "from" of a Become line.
pub const FROM_TOOLTIP: &str = "Only cells in this state now change";

/// Returns a state's name as chips and pickers show it: `state`, or "Status Alive" with `property` when another
/// property has a state of the same name.
pub fn shared_state_name(property: &str, state: &str, shared: bool) -> String {
    if shared {
        format!("{property} {state}")
    } else {
        state.to_owned()
    }
}

// Pickers

/// Number box of a picker of a number.
pub const NUMBER_BOX_HINT: &str = "Type a number, then press Enter.";

/// Number box of a picker of a chance.
pub const CHANCE_BOX_HINT: &str = "Type a chance such as 0.3 or 30%, then press Enter.";

/// Button beside the Number box.
pub const USE_BUTTON: &str = "Use";

/// Heading of the group of the cell's properties in a picker of a number.
pub const THIS_CELLS_PROPERTIES: &str = "This cell's properties";

/// Returns an item of the group of the cell's properties, such as "my Food".
pub fn my_property_item(property: &str) -> String {
    format!("my {property}")
}

/// Returns the grey words beside an item of the group of the cell's properties, such as "this cell's Food".
pub fn this_cells_property(property: &str) -> String {
    format!("this cell's {property}")
}

/// Heading of the group of parameters in a picker of a number.
pub const PARAMETERS: &str = "Parameters";

/// Last item of a picker of a number, which opens the New parameter dialog.
pub const NEW_PARAMETER: &str = "New parameter...";

/// Heading of the group of states in a picker of a state.
pub const STATES: &str = "States";

/// First item of a picker of an old state.
pub const ANY_STATE: &str = "any state";

/// Heading of the group of properties in a picker of a property.
pub const PROPERTIES: &str = "Properties";

/// Heading of the list blank's box.
pub const LIST_GROUP: &str = "Numbers, separated by commas";

/// Hint of the list blank's box.
pub const LIST_HINT: &str = "Type one or more numbers, such as 2, 3, then press Enter.";

/// Item of the picker of a filled list item.
pub const REMOVE_FROM_LIST: &str = "Remove from list";

/// Returns the lead of the "for each" picker of a draw.
pub fn for_each_lead(draw: DrawKind) -> &'static str {
    match draw {
        DrawKind::Rate => {
            "The rate grows with what you choose. With for each infected neighbour, a cell with 2 infected neighbours \
             gets twice the rate."
        }
        DrawKind::Chance => "Each one counted is a separate try with this chance.",
    }
}

/// Item of the "for each" picker once "for each" is on.
pub const REMOVE_FOR_EACH: &str = "Remove for each";

// Messages of a picker with nothing to offer

/// No number result above the line.
pub const NO_NUMBER_RESULT: &str = "No number result above this line yet.";
/// No property for the value left of a comparison.
pub const NO_PROPERTY: &str = "No property yet.";
/// No Number property for a number.
pub const NO_NUMBER_PROPERTY: &str = "No Number property yet.";
/// No States property for a picker of a property.
pub const NO_STATES_PROPERTY: &str = "No States property yet. Add one under Cells on the sheet.";
/// No Number property for a picker of a property.
pub const NO_NUMBER_PROPERTY_ON_SHEET: &str = "No Number property yet. Add one under Cells on the sheet.";
/// No yes or no result above the line.
pub const NO_YES_OR_NO_RESULT: &str = "No yes or no result above this line yet. Make one with + Yes or no above this \
                                       line.";
/// No state for a picker of a state.
pub const NO_STATES: &str = "No states yet. Add a States property under Cells on the sheet.";

// Messages for a typed number

/// A typed chance outside 0 to 1.
pub const CHANCE_OUT_OF_RANGE: &str = "A chance is from 0 to 1, or from 0% to 100%.";
/// A typed rate below 0.
pub const RATE_BELOW_ZERO: &str = "A rate cannot be below 0.";
/// Typed text that is not one number.
pub const TYPE_ONE_NUMBER: &str = "Type one number, such as 3 or 0.25.";
/// A list typed in a blank of one number.
pub const LIST_IN_ONE_NUMBER: &str = "For a list such as 2, 3, choose is one of the numbers or is none of the numbers \
                                      first.";

/// Title of the colour palette.
pub const CHOOSE_A_COLOR: &str = "Choose a colour";

/// Names of the colours of the palette, in order.
pub const COLOR_NAMES: [&str; 12] = [
    "Black",
    "Dark grey",
    "Grey",
    "White",
    "Red",
    "Orange",
    "Yellow",
    "Brown",
    "Green",
    "Dark green",
    "Blue",
    "Purple",
];

// Result names

/// Returns the automatic name of a Count line that counts `state`: "Alive neighbours", or "B in my block" for the 3 by
/// 3 block.
pub fn count_result_name(state: &str, block: bool) -> String {
    if block {
        format!("{state} in my block")
    } else {
        format!("{state} neighbours")
    }
}

/// Automatic name of a Count line while its state is empty.
pub const COUNT_RESULT_EMPTY: &str = "Count";

/// Returns the automatic name of a Total or average line over `property`: "Total Food around" or "Average Food
/// around".
pub fn total_result_name(property: &str, average: bool) -> String {
    if average {
        format!("Average {property} around")
    } else {
        format!("Total {property} around")
    }
}

/// Returns the automatic name of a chance or rate result whose value has the base `base`, such as "Recovery happens".
pub fn happens_result_name(base: &str) -> String {
    format!("{base} happens")
}

/// Automatic name of a chance or rate result while its value is empty.
pub const HAPPENS: &str = "Happens";

/// Returns the base a parameter named `name` gives a chance or rate result: the name without a last word "rate",
/// "chance" or "probability", such as "Recovery" for Recovery rate.
pub fn happens_base(name: &str) -> &str {
    for word in [" rate", " chance", " probability"] {
        if let Some(base) = name.strip_suffix(word)
            && !base.is_empty()
        {
            return base;
        }
    }
    name
}

/// Returns the base a typed rate gives a rate result, such as "0.1 per day".
pub fn typed_rate_base(number: &str, unit: TimeUnit) -> String {
    format!("{number} per {}", rate_unit_word(unit))
}

/// Returns the base a typed chance gives a chance result, such as "0.3 chance" or "30% chance".
pub fn typed_chance_base(number: &str) -> String {
    format!("{number} chance")
}

/// Automatic name of a Compare line while it is empty.
pub const TEST_RESULT_EMPTY: &str = "Test";

/// Automatic name of a Calculate line while it is empty.
pub const CALCULATE_RESULT_EMPTY: &str = "Result";

/// Automatic name of an all of line.
pub const ALL_OF_RESULT: &str = "All of";

/// Automatic name of an any of line.
pub const ANY_OF_RESULT: &str = "Any of";

/// Returns `name` made unique by its `count`th use, such as "Alive neighbours 2", or `name` itself for the first use.
pub fn repeated_result_name(name: &str, count: usize) -> String {
    if count <= 1 {
        name.to_owned()
    } else {
        format!("{name} {count}")
    }
}

// Notes under a line, in the order a line shows them

/// Returns the skip note of a line whose first empty blank wants `want`, such as "Not run yet: choose a state", where
/// `want` is [`Blank::want`] or [`LineSelect::words`].
pub fn not_run_choose(want: &str) -> String {
    format!("Not run yet: choose {want}")
}

/// Returns the skip note of a line that uses the result `name` of a line below it.
pub fn uses_result_below(name: &str) -> String {
    format!("Uses {name}, which is below this line")
}

/// Returns the skip note of a line that uses the result `name` from inside When line `number`, which does not hold
/// the line.
pub fn uses_result_inside(name: &str, number: usize) -> String {
    format!("Uses {name} from inside line {number}. Only lines inside line {number} can use it. Choose another value.")
}

/// Returns the skip note of a line that uses the result `name` of line `number`, which does not run.
pub fn not_run_result_unfinished(name: &str, number: usize) -> String {
    format!("Not run yet: {name} on line {number} is not finished")
}

/// Skip note of a rate while one tick has no length.
pub const NOT_RUN_TICK: &str = "Not run yet: set how long one tick is, under World on the sheet";

/// Returns the skip note of a line inside When line `number`, which does not run.
pub fn not_run_when_unfinished(number: usize) -> String {
    format!("Not run yet: line {number} is not finished")
}

/// Returns the tooltip of the red chip of the result `name` of a line below.
pub fn result_below_tooltip(name: &str) -> String {
    format!("{name} is below this line. Move this line under it, or choose another value.")
}

/// Returns the tooltip of the red chip of the result `name` from inside a When that does not hold the line.
pub fn result_inside_tooltip(name: &str) -> String {
    format!("{name} belongs to the lines inside a When this line is not in. Choose another value.")
}

/// Note under an outside test with one side filled.
pub const ONE_SIDED_RANGE: &str = "For one side only, a plain comparison reads better.";

/// Returns the note under a rate line whose unit differs from the unit of its parameter, such as "Infection rate is
/// per day on the sheet, but this line reads it per week."
pub fn unit_mismatch(parameter: &str, sheet_unit: TimeUnit, line_unit: TimeUnit) -> String {
    format!(
        "{parameter} is per {} on the sheet, but this line reads it per {}.",
        rate_unit_word(sheet_unit),
        rate_unit_word(line_unit)
    )
}

/// Returns the note under a draw whose value is the count `count`, where `each` is the count's words after "for each".
///
/// For example "Infected neighbours is a count, not a rate. Did you mean a rate for each infected neighbour?"
pub fn count_used_as_draw(count: &str, draw: DrawKind, each: &str) -> String {
    let word = draw.word();
    format!("{count} is a count, not a {word}. Did you mean a {word} for each {each}?")
}

/// Returns the "for each" prompt under a fixed rate, such as "Should this rate grow with the number of Infected
/// neighbours?"
pub fn rate_grow_prompt(count: &str) -> String {
    format!("Should this rate grow with the number of {count}?")
}

/// Returns the values of a count as notes list them: single values joined with "or", and a run of three or more as
/// "4 to 8".
///
/// `values` must be in increasing order.
pub fn count_values_words(values: &[u32]) -> String {
    let mut runs: Vec<String> = Vec::new();
    let mut index = 0;
    while index < values.len() {
        let mut end = index;
        while end + 1 < values.len() && values[end + 1] == values[end] + 1 {
            end += 1;
        }
        if end - index >= 2 {
            runs.push(format!("{} to {}", values[index], values[end]));
            index = end + 1;
        } else {
            runs.push(values[index].to_string());
            index += 1;
        }
    }
    match runs.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// Returns the gap note under the last of the Become lines `lines`, which test the count `count`.
///
/// `values` are the values of the count that no line applies to, and `property` is the property those cells keep. For
/// example "None of lines 2 and 3 applies when B in my block is 4 or 5. Those cells keep their Opinion."
pub fn gap_note(lines: &[usize], count: &str, values: &[u32], property: &str) -> String {
    format!(
        "None of lines {} applies when {count} is {}. Those cells keep their {property}.",
        number_list(lines),
        count_values_words(values)
    )
}

/// Returns the coverage note under the last of the Become lines `lines` that together set every value of the count
/// `count`, where `ends` holds each end state with the values of the count that give it.
///
/// For example "Lines 2 and 3 cover every value of B in my block. A cell ends the tick A when it is 0 to 4 and B when
/// it is 5 to 9."
pub fn coverage_note(lines: &[usize], count: &str, ends: &[(&str, &[u32])]) -> String {
    let ends: Vec<String> = ends
        .iter()
        .map(|(state, values)| format!("{state} when it is {}", count_values_words(values)))
        .collect();
    format!(
        "Lines {} cover every value of {count}. A cell ends the tick {}.",
        number_list(lines),
        word_list(&ends)
    )
}

/// Returns the order note under the last of the Become lines `lines`, which change cells in the old states `states`.
///
/// For example "Lines 2 and 3 change different cells (Dead and Alive), so their order does not matter."
pub fn order_note(lines: &[usize], states: &[&str]) -> String {
    let states: Vec<String> = states.iter().map(|state| (*state).to_owned()).collect();
    format!(
        "Lines {} change different cells ({}), so their order does not matter.",
        number_list(lines),
        word_list(&states)
    )
}

/// Returns the note under a When whose lines all change from `state`, which the line `number` already says.
pub fn when_repeats_note(number: usize, state: &str) -> String {
    format!("Line {number} already says from {state}, so this When repeats it.")
}

/// Label of the button that removes a When and keeps its lines.
pub const REMOVE_THIS_WHEN: &str = "Remove this When";

/// Returns the accessible name of the button that removes When line `number`.
pub fn remove_when_name(number: usize) -> String {
    format!("Remove line {number} and keep the lines inside")
}

/// Returns the note under a When around the one change on line `number`.
pub fn only_one_inside_note(number: usize) -> String {
    format!("Only line {number} is inside. Its own when can hold this condition.")
}

/// Returns the label of the button that moves a When's condition into the change on line `number`.
pub fn move_into_label(number: usize) -> String {
    format!("Move it into line {number}")
}

/// Returns the accessible name of the button that moves the condition of When line `when` into line `inside`.
pub fn move_into_name(when: usize, inside: usize) -> String {
    format!("Move the condition of line {when} into line {inside}")
}

/// Returns the note under a change or When inside When line `number`, whose condition reads `condition`, such as
/// "Only when line 4 holds: my Health is Recovered."
pub fn only_when_note(number: usize, condition: &str) -> String {
    format!("Only when line {number} holds: {condition}.")
}

/// Returns the note under a result line inside When line `number`.
pub fn worked_out_everywhere_note(number: usize) -> String {
    format!("Worked out for every cell, whatever line {number} says. Only lines inside line {number} can use it.")
}

/// Note under a When that holds no line.
pub const NOTHING_INSIDE: &str = "Nothing inside yet.";

// "otherwise"

/// Returns the words before a change that a higher line overrides, such as "otherwise (when line 2 does not apply)",
/// where `lines` holds the numbers of the higher lines.
pub fn otherwise_words(lines: &[usize]) -> String {
    format!("{OTHERWISE} ({})", otherwise_description(lines))
}

/// Returns the tooltip of the "otherwise" words, where `lines` holds the higher lines, `property` the property they
/// change and `state` the state a cell must be in for both to apply.
///
/// For example "Line 2 also changes Cover for a cell that is Tree. When both apply, line 2 wins."
pub fn otherwise_tooltip(lines: &[usize], property: &str, state: Option<&str>) -> String {
    let who = if lines.len() == 1 {
        format!("Line {} also changes", number_list(lines))
    } else {
        format!("Lines {} also change", number_list(lines))
    };
    let for_whom = state
        .map(|state| format!(" for a cell that is {state}"))
        .unwrap_or_default();
    let wins = if lines.len() == 1 {
        format!("When both apply, line {} wins.", number_list(lines))
    } else {
        "When more than one applies, the highest line wins.".to_owned()
    };
    format!("{who} {property}{for_whom}. {wins}")
}

/// Returns the notice shown the first time in a session that line `number` starts with "otherwise", followed by the
/// tooltip of [`otherwise_tooltip`].
pub fn otherwise_notice(number: usize, lines: &[usize], property: &str, state: Option<&str>) -> String {
    format!(
        "Line {number} now starts with otherwise. {}",
        otherwise_tooltip(lines, property, state)
    )
}

// Rates and chances

/// Returns the start of a rate note: "Each tick", or "Each tick (1 day)" when one tick has the length `length`.
pub fn each_tick(length: Option<&str>) -> String {
    match length {
        Some(length) => format!("Each tick ({length})"),
        None => "Each tick".to_owned(),
    }
}

/// Returns the subject of a rate note on a Become line from `old_state` to `new_state`.
///
/// It reads "a Susceptible cell becomes Infected" ("an" before a vowel), "a cell becomes Infected" from any state or
/// with the old state empty, or "a cell changes" with the new state empty.
pub fn become_subject(old_state: Option<&str>, new_state: Option<&str>) -> String {
    match (old_state, new_state) {
        (_, None) => "a cell changes".to_owned(),
        (None, Some(new)) => format!("a cell becomes {new}"),
        (Some(old), Some(new)) => format!("{} {old} cell becomes {new}", indefinite_article(old)),
    }
}

/// Subject of a rate note on a Set line.
pub const SET_SUBJECT: &str = "a cell takes the new value";

/// Returns the subject of a rate note on When line `number`.
pub fn when_subject(number: usize) -> String {
    format!("the lines inside line {number} apply to a cell")
}

/// Returns the subject of a rate note on the result `name`.
pub fn result_subject(name: &str) -> String {
    format!("the result {name} is yes for a cell")
}

/// Returns the notice shown when a rate's unit sets one tick, such as "One tick is now 1 day. Change it on the sheet."
pub fn tick_set_notice(unit: TimeUnit) -> String {
    format!("One tick is now 1 {}. Change it on the sheet.", rate_unit_word(unit))
}

/// Returns a count's name as it reads after "for each": "infected neighbour" for Infected neighbours.
///
/// A name whose last word is not a plural keeps its words after "of" ("of B in my block"), and so does a name starting
/// with "my". An ordinary capitalised first word drops its capital, and a one-letter or all-capital word keeps it ("B
/// neighbour").
pub fn for_each_words(name: &str) -> String {
    if name.starts_with("my ") {
        return format!("of {name}");
    }
    let (head, last) = name.rsplit_once(' ').unwrap_or(("", name));
    let bytes = last.as_bytes();
    let plural = bytes.len() >= 2 && bytes[bytes.len() - 1] == b's' && bytes[bytes.len() - 2] != b's';
    if !plural {
        return format!("of {name}");
    }
    let singular = &last[..last.len() - 1];
    if head.is_empty() {
        drop_ordinary_capital(singular)
    } else {
        drop_ordinary_capital(&format!("{head} {singular}"))
    }
}

/// First sentence after the chance of a rate note.
pub const RATE_IS_NOT_A_CHANCE: &str = "A rate is not a chance.";

/// Returns the chances of a line counted for each `count`, at 1 and at 2, for a note.
///
/// It reads "with chance 0.259 with 1 infected neighbour, and 0.451 with 2", or "with chance 0.259 when B in my block
/// is 1, and 0.451 when it is 2" when the count's name has no singular form. `first` and `second` are the chances at 1
/// and at 2, already formatted.
pub fn count_chances(count: &str, first: &str, second: &str) -> String {
    let each = for_each_words(count);
    if each.starts_with("of ") {
        format!("with chance {first} when {count} is 1, and {second} when it is 2")
    } else {
        format!("with chance {first} with 1 {each}, and {second} with 2")
    }
}

/// Returns the note of a rate line, such as "Each tick (1 day), an Infected cell becomes Recovered with chance 0.0952.
/// A rate is not a chance. Henad turns it into one as 1 − e^(−0.1 × 1 day)."
///
/// `tick` is the length of one tick, `subject` the note's subject from [`become_subject`] and the like, and `chance`
/// and `rate` are formatted numbers.
pub fn rate_note(tick: &str, subject: &str, chance: &str, rate: &str) -> String {
    format!(
        "{}, {subject} with chance {chance}. {RATE_IS_NOT_A_CHANCE} Henad turns it into one as 1 − e^(−{rate} × \
         {tick}).",
        each_tick(Some(tick))
    )
}

/// Returns the note of a rate line counted for each `count`, with the chances `first` and `second` at 1 and at 2.
///
/// For example "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259 with 1 infected neighbour,
/// and 0.451 with 2. A rate is not a chance. Henad turns it into one as 1 − e^(−0.3 × n × 1 day), where n is Infected
/// neighbours."
pub fn rate_for_each_note(tick: &str, subject: &str, count: &str, first: &str, second: &str, rate: &str) -> String {
    format!(
        "{}, {subject} {}. {RATE_IS_NOT_A_CHANCE} Henad turns it into one as 1 − e^(−{rate} × n × {tick}), where n is \
         {count}.",
        each_tick(Some(tick)),
        count_chances(count, first, second)
    )
}

/// Returns the note of a rate line whose rate is a Calculate result of a fixed number times the count `count`.
///
/// `rate_words` reads the result, as "0.3 times Infected neighbours". For example "Each tick (1 day), a Susceptible
/// cell becomes Infected with chance 0.259 with 1 infected neighbour, and 0.451 with 2. A rate is not a chance. Henad
/// turns it into one as 1 − e^(−r × 1 day), where r is 0.3 times Infected neighbours."
pub fn worked_out_rate_note(
    tick: &str,
    subject: &str,
    count: &str,
    first: &str,
    second: &str,
    rate_words: &str,
) -> String {
    format!(
        "{}, {subject} {}. {RATE_IS_NOT_A_CHANCE} Henad turns it into one as 1 − e^(−r × {tick}), where r is \
         {rate_words}.",
        each_tick(Some(tick)),
        count_chances(count, first, second)
    )
}

/// Returns the note of a rate line whose rate is any other worked-out result, read as `rate_words`, and counted for
/// each `count` when there is one.
///
/// For example "Each tick (1 day), a Susceptible cell becomes Infected with a chance that grows with r, where r is
/// Infected neighbours plus 0.3. A rate is not a chance. Henad turns it into one as 1 − e^(−r × 1 day)."
pub fn growing_rate_note(tick: &str, subject: &str, rate_words: &str, count: Option<&str>) -> String {
    let head = each_tick(Some(tick));
    match count {
        None => format!(
            "{head}, {subject} with a chance that grows with r, where r is {rate_words}. {RATE_IS_NOT_A_CHANCE} Henad \
             turns it into one as 1 − e^(−r × {tick})."
        ),
        Some(count) => format!(
            "{head}, {subject} with a chance that grows with r and n, where r is {rate_words} and n is {count}. \
             {RATE_IS_NOT_A_CHANCE} Henad turns it into one as 1 − e^(−r × n × {tick})."
        ),
    }
}

/// Returns the note of a chance line counted for each `count`, with the chances `first` and `second` at 1 and at 2.
///
/// `tick` is the length of one tick, if it has one. For example "Each tick, a cell becomes B with chance 0.2 with 1 B
/// neighbour, and 0.36 with 2. Each one counted is a separate try: 1 − (1 − 0.2)^n, where n is B neighbours." A
/// chance without "for each" has no note.
pub fn chance_for_each_note(tick: Option<&str>, subject: &str, count: &str, first: &str, second: &str) -> String {
    format!(
        "{}, {subject} {}. Each one counted is a separate try: 1 − (1 − {first})^n, where n is {count}.",
        each_tick(tick),
        count_chances(count, first, second)
    )
}

/// Returns the note of a chance line counted for each `count` whose chance is a result, read as `chance_words`.
///
/// For example "Each tick, a cell becomes B with a chance that grows with n, where p is Spark chance and n is B
/// neighbours. Each one counted is a separate try: 1 − (1 − p)^n."
pub fn growing_chance_note(tick: Option<&str>, subject: &str, chance_words: &str, count: &str) -> String {
    format!(
        "{}, {subject} with a chance that grows with n, where p is {chance_words} and n is {count}. Each one counted \
         is a separate try: 1 − (1 − p)^n.",
        each_tick(tick)
    )
}

/// Returns the note of a group of competing rate lines that take cells out of `state`.
///
/// `chance` is the chance of leaving, `shares` holds each new state with its share, and `rates` each line's rate, all
/// formatted. For example "Each tick (1 day), an Infected cell leaves Infected with chance 0.0952: Recovered 0.0476,
/// Dead 0.0476. A rate is not a chance. Henad turns it into one as 1 − e^(−(0.05 + 0.05) × 1 day)."
pub fn competing_note(tick: &str, state: &str, chance: &str, shares: &[(&str, &str)], rates: &[&str]) -> String {
    let shares: Vec<String> = shares
        .iter()
        .map(|(new_state, share)| format!("{new_state} {share}"))
        .collect();
    format!(
        "{}, {} {state} cell leaves {state} with chance {chance}: {}. {RATE_IS_NOT_A_CHANCE} Henad turns it into one \
         as 1 − e^(−({}) × {tick}).",
        each_tick(Some(tick)),
        indefinite_article(state),
        shares.join(", "),
        rates.join(" + ")
    )
}

/// Returns the largest chance of a rate counted for each, on a tick that is long for it, such as "up to 0.91 with 8".
pub fn long_tick_chance(chance: &str, count: u32) -> String {
    format!("up to {chance} with {count}")
}

/// Returns the description of a rate note on how long a cell stays in `state`.
///
/// `stay` is the mean stay at this tick length and `limit` the mean stay as the tick shrinks, both with their unit,
/// and `rate` the rate. For example "On average a cell stays Infected 10.5 days at this tick length. It approaches
/// 1/0.1 = 10 days as the tick shrinks."
pub fn mean_stay_note(state: &str, stay: &str, rate: &str, limit: &str) -> String {
    format!(
        "On average a cell stays {state} {stay} at this tick length. It approaches 1/{rate} = {limit} as the tick \
         shrinks."
    )
}

// Errors on a rate in a button's card and on a second agent kind, competing rates, the long tick and rate
// conversion

/// Error on a rate in the card of a button.
pub const RATE_IN_BUTTON_CARD: &str = "A button's card has no duration, so it takes chances, not rates.";

/// Error on a second kind of agent.
pub const ONE_AGENT_KIND: &str = "One kind of agent per model in this version.";

// Errors on the lines and worlds a sheet can hold that this version cannot run yet

/// Error on a line that holds a rate.
pub const NOT_YET_RATE: &str = "Rates are not supported yet.";

/// Error on a Repeat line.
pub const NOT_YET_REPEAT: &str = "Repeat is not supported yet.";

/// Error on a Become, Set, Keep between, When or Repeat line inside a Repeat.
pub const NOT_YET_REPEAT_CHANGE: &str =
    "Become, Set, Keep between, When and Repeat inside a Repeat are not supported yet.";

/// Error on the ground under agents, a look at the cells around an agent and a start on the ground.
pub const NOT_YET_GROUND: &str = "The ground is not supported yet.";

/// Error on a Pick a cell line.
pub const NOT_YET_PICK_CELL: &str = "Pick a cell is not supported yet.";

/// Error on a line that looks at the cell here or ahead.
pub const NOT_YET_LOOK_AT_CELL: &str = "Looking at the cell here or ahead is not supported yet.";

/// Error on a Move to line.
pub const NOT_YET_MOVE_TO: &str = "Move to is not supported yet.";

/// Error on a Leave line.
pub const NOT_YET_LEAVE: &str = "Leave is not supported yet.";

/// Error on a Record line and a count of events over agents.
pub const NOT_YET_RECORD: &str = "Record is not supported yet.";

/// Error on a world of agents with walls at its edges.
pub const NOT_YET_AGENT_WALLS: &str = "Walls around agents in open space are not supported yet.";

// Errors on a change of a property in a second way, and the words of a look at agents that costs too much

/// Returns the error on a Steer of a property that line `line` sets: "Line 3 sets Velocity. A card that sets a
/// property cannot also steer it."
///
/// `line` is the line prefix, such as "Line 3".
pub fn set_already(line: &str, property: &str) -> String {
    format!("{line} sets {property}. A card that sets a property cannot also steer it.")
}

/// Returns the error on a Set of a property that line `line` steers: "Line 3 steers Velocity. A card that steers a
/// property cannot also set it."
pub fn steered_already(line: &str, property: &str) -> String {
    format!("{line} steers {property}. A card that steers a property cannot also set it.")
}

/// Returns the error on a Move to below line `line`, a line that moves by an arrow.
pub fn moved_by_already(line: &str) -> String {
    format!("{line} moves by an arrow. A card that moves by an arrow cannot also move to a place.")
}

/// Returns the error on a Move by below line `line`, a line that moves to a place.
pub fn moved_to_already(line: &str) -> String {
    format!("{line} moves to a place. A card that moves to a place cannot also move by an arrow.")
}

/// Returns the name of the looks at agents of the lines numbered in `lines`, each with its name when it has one: "line
/// 3, Flockmates", or "lines 1 and 2, Flockmates and Too close".
pub fn look_name(lines: &[(usize, Option<&str>)]) -> String {
    let numbers: Vec<usize> = lines.iter().map(|(number, _)| *number).collect();
    let names: Vec<String> = lines.iter().filter_map(|(_, name)| name.map(str::to_owned)).collect();
    let mut words = if numbers.len() == 1 {
        format!("line {}", numbers[0])
    } else {
        format!("lines {}", number_list(&numbers))
    };
    if !names.is_empty() {
        words.push_str(", ");
        words.push_str(&word_list(&names));
    }
    words
}

/// Returns the error on looks at agents named `look` that cost too much at `count` agents.
///
/// `things` is the plural of the kind of agent. For example "Line 1, Flockmates: looks at too many birds at 1,000,000
/// birds. A shorter distance or fewer birds would fit."
pub fn look_costs_too_much(look: &str, things: &str, count: u64) -> String {
    format!(
        "{}: looks at too many {things} at {} {things}. A shorter distance or fewer {things} would fit.",
        upper_first(look),
        whole_number_text(count)
    )
}

/// Returns the error on the card headed `heading` that costs too much at `count` agents.
///
/// `things` is the plural of the kind of agent. For example "Each tick, every bird: too much work at 1,000,000 birds.
/// Fewer birds would fit."
pub fn card_costs_too_much(heading: &str, things: &str, count: u64) -> String {
    format!(
        "{heading}: too much work at {} {things}. Fewer {things} would fit.",
        whole_number_text(count)
    )
}

/// Returns the error on the line named `line` whose work for one `thing` in one run of its card passes the limit,
/// with `fix` the change that would fit.
///
/// `line` is a name as [`look_name`] writes it. For example "Line 1: too much work for one walker. A smaller maximum
/// for Steps per tick would fit."
pub fn line_costs_too_much(line: &str, thing: &str, fix: &str) -> String {
    format!("{}: too much work for one {thing}. {fix}", upper_first(line))
}

/// Returns the change to the Repeat over the parameter named `parameter` that would fit: "A smaller maximum for Steps
/// per tick would fit."
pub fn smaller_maximum(parameter: &str) -> String {
    format!("A smaller maximum for {parameter} would fit.")
}

/// Change to a Repeat of a fixed number of times that would fit.
pub const FEWER_TIMES: &str = "A Repeat of fewer times would fit.";

/// Change to a card or a line other than a Repeat that would fit.
pub const FEWER_LINES: &str = "Fewer lines on this card would fit.";

/// Returns the error on the starts of `count` agents that cost too much, `things` the plural of the kind of agent:
/// "Starting 1,000,000 birds takes too much work. Fewer birds would fit."
pub fn start_costs_too_much(things: &str, count: u64) -> String {
    format!(
        "Starting {} {things} takes too much work. Fewer {things} would fit.",
        whole_number_text(count)
    )
}

/// Returns `count` with a comma between each group of three digits, such as "1,000,000".
fn whole_number_text(count: u64) -> String {
    let digits = count.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// Word that starts the lower of two rate lines that compete, in place of "otherwise".
pub const COMPETING_WORD: &str = "or";

/// Returns the tooltip of the word of competing rate lines `lines`, which take cells out of `state`.
pub fn competing_tooltip(lines: &[usize], state: &str) -> String {
    format!(
        "Lines {} compete. A cell leaves {state} at the sum of their rates, and each line takes its share.",
        number_list(lines)
    )
}

/// Note on a tick that is long for its rate.
pub const LONG_TICK: &str = "This tick is long for this rate. A shorter tick follows the rate more closely.";

/// Item of a line's menu that turns its chance into a rate.
pub const CHANGE_TO_RATE: &str = "Change to a rate";

/// Item of a line's menu that turns its rate into a chance.
pub const CHANGE_TO_CHANCE: &str = "Change to a chance";

/// Returns the note under the Time line of the sheet about the lines `lines`, which use chances per tick.
pub fn chances_per_tick_note(lines: &[usize]) -> String {
    let (word, verb) = if lines.len() == 1 {
        ("Line", "uses")
    } else {
        ("Lines", "use")
    };
    format!(
        "{word} {} {verb} chances per tick. They do not follow the time scale.",
        number_list(lines)
    )
}

// The Lines help

/// Text of the Lines help.
pub const LINES_HELP: &str = "Every cell follows these lines each tick, from the top. A line either works out a \
                              result, which lines below it can use, or changes one of my properties. Everyone reads \
                              the world as it was at the start of the tick, and all changes land together at the end \
                              of it. Whatever no line changes stays as it was. When two changes to one property both \
                              apply, the higher line wins, and the lower one shows \"otherwise\".";

/// Example of the Lines help.
pub const LINES_HELP_EXAMPLE: &str = "Alive neighbours = count of the 8 cells around me whose Status is Alive";

/// Label before the example of a help popover.
pub const HELP_EXAMPLE_LABEL: &str = "Example:";

/// Button that closes a help popover.
pub const CLOSE_HELP: &str = "Close help";

/// Words the card and the text a sheet produces never show, outside the controls that open a node graph.
pub const FORBIDDEN_WORDS: [&str; 19] = [
    "pass",
    "stage",
    "lane",
    "domain",
    "IR",
    "shader",
    "WGSL",
    "kernel",
    "buffer",
    "node",
    "port",
    "wire",
    "category",
    "variant",
    "seed",
    "stream",
    "accumulator",
    "region",
    "compile",
];

// The sheet

/// One section of the sheet, with its heading and its help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetSection {
    /// What the model is for.
    Purpose,
    /// The grid, its edges and the length of one tick.
    World,
    /// The properties every cell has.
    Cells,
    /// The named numbers lines can use.
    Parameters,
    /// The numbers drawn on the plot.
    Measures,
    /// The property that colours the world.
    Colours,
    /// The buttons beside the world.
    Buttons,
}

impl SheetSection {
    /// Every section, in the order the sheet shows them.
    pub const ALL: [Self; 7] = [
        Self::Purpose,
        Self::World,
        Self::Cells,
        Self::Parameters,
        Self::Measures,
        Self::Colours,
        Self::Buttons,
    ];

    /// Heading of the section, such as "Purpose".
    pub fn heading(self) -> &'static str {
        match self {
            Self::Purpose => "Purpose",
            Self::World => "World",
            Self::Cells => "Cells",
            Self::Parameters => "Parameters",
            Self::Measures => "Measures",
            Self::Colours => "Colours",
            Self::Buttons => "Buttons",
        }
    }

    /// Returns the accessible name of the "?" beside the heading, such as "Help: Purpose".
    pub fn help_name(self) -> String {
        format!("Help: {}", self.heading())
    }

    /// Text of the section's help popover.
    pub fn help(self) -> &'static str {
        match self {
            Self::Purpose => "Say in a sentence what the model is for and what to look for when it runs.",
            Self::World => {
                "The grid the cells live on: its size, what happens at its edges, and how much time one tick stands \
                 for. A tick is one step of time, and every cell follows the card once per tick. Leave one tick as \
                 one step (no unit) when time has no unit, as in Game of Life. With Wrap around, a cell on the left \
                 edge has neighbours on the right edge."
            }
            Self::Cells => {
                "A property is something every cell has. A States property is one of a few named states, such as Dead \
                 or Alive. A Number property holds a number. Starts as says what every cell holds when the run starts."
            }
            Self::Parameters => {
                "A parameter is a named number with a slider. Lines can use it. The value here is the one the model \
                 keeps. The slider beside the world tries other values in the run without changing the model."
            }
            Self::Measures => {
                "A measure is a number worked out from all the cells every tick, drawn on the plot under the world. \
                 Tick Plot beside a state to count the cells in it."
            }
            Self::Colours => {
                "Choose which property colours the cells in the world. A States property uses the colours of its \
                 states. A Number property is shaded from dark to light."
            }
            Self::Buttons => {
                "A button appears beside the world. Start again redraws how cells start. Run once changes every cell \
                 one time, with its own lines."
            }
        }
    }

    /// Example of the section's help popover, shown after [`HELP_EXAMPLE_LABEL`].
    pub fn help_example(self) -> &'static str {
        match self {
            Self::Purpose => "Spread of a disease through a town. Look for waves of infection.",
            Self::World => "One tick is 1 day",
            Self::Cells => "Status: States Dead, Alive. Starts as Alive with chance 0.3, otherwise Dead",
            Self::Parameters => "Infection rate = 0.3 per day",
            Self::Measures => "Number of cells whose Status is Alive",
            Self::Colours => "Colour cells by Status",
            Self::Buttons => "Clear: Become Dead from any state, always",
        }
    }
}

/// Label of a name field, and the first column of the Cells and Parameters tables.
pub const NAME_LABEL: &str = "Name";

/// Label of the kind of a property.
pub const KIND_LABEL: &str = "Kind";

/// Button that keeps a form.
pub const DONE: &str = "Done";

/// Button that closes a form or a dialog without a change.
pub const CANCEL: &str = "Cancel";

/// Placeholder of the Purpose field, which takes the heading as its accessible name.
pub const PURPOSE_PLACEHOLDER: &str = "What is this model for?";

/// What the world is made of, in the World section and in the New model dialog.
pub const CELLS_ON_A_GRID: &str = "Cells on a grid";

/// Label of the size of the grid.
pub const SIZE_LABEL: &str = "Size";

/// Word between the width and the height of the grid.
pub const SIZE_BY: &str = "by";

/// Accessible name of the width field.
pub const WIDTH_NAME: &str = "Width in cells";

/// Accessible name of the height field.
pub const HEIGHT_NAME: &str = "Height in cells";

/// Label of the edges of the grid.
pub const EDGES_LABEL: &str = "Edges";

/// Edges that join the opposite edge.
pub const WRAP_AROUND: &str = "Wrap around";

/// Edges that end the grid.
pub const WALLS: &str = "Walls";

/// Sentence under the edges while they wrap around.
pub const WRAP_SENTENCE: &str = "The left edge joins the right edge, and the top edge joins the bottom.";

/// Sentence under the edges while they are walls, and the tooltip of [`WALLS`].
pub const WALLS_SENTENCE: &str = "Cells on an edge have fewer cells around them.";

/// Tooltip of [`WRAP_AROUND`].
pub const WRAP_TOOLTIP: &str =
    "Cells on the left edge are neighbours of cells on the right edge, and the top edge joins the bottom.";

/// Label before the length of one tick.
pub const ONE_TICK_IS: &str = "One tick is";

/// Returns the choice of the tick's unit: "one step (no unit)" for no unit, or the unit's plural, such as "days".
pub fn tick_unit_choice(unit: Option<TimeUnit>) -> &'static str {
    unit.map_or("one step (no unit)", TimeUnit::plural)
}

/// Accessible name of the length of one tick, hidden while the tick has no unit.
pub const TICK_LENGTH_NAME: &str = "Length of one tick";

/// Accessible name of the unit of one tick.
pub const TICK_UNIT_NAME: &str = "Unit of one tick";

/// Columns of the Cells table.
pub const CELLS_COLUMNS: [&str; 4] = [NAME_LABEL, KIND_LABEL, "Starts as", "Changed by"];

/// Returns the Changed by column of a property that the lines `lines` change: "lines 2, 3", "line 2" or "no line
/// yet".
pub fn changed_by(lines: &[usize]) -> String {
    let numbers: Vec<String> = lines.iter().map(usize::to_string).collect();
    match lines.len() {
        0 => "no line yet".to_owned(),
        1 => format!("line {}", numbers.join(", ")),
        _ => format!("lines {}", numbers.join(", ")),
    }
}

/// Text of the Cells section of a sheet with no property, above [`ADD_PROPERTY`] as the primary action.
pub const SHEET_EMPTY: &str = "Every cell has the properties listed here. Add the first one to start.";

/// Notice that a change to the sheet after the first tick starts the world again.
pub const RESTART_NOTICE: &str = "Changes restart the world from tick 0.";

// The property form

/// Button that opens the property form.
pub const ADD_PROPERTY: &str = "+ Add property";

/// Heading of the property form.
pub const NEW_PROPERTY: &str = "New property";

/// Placeholder of the property's name.
pub const PROPERTY_NAME_PLACEHOLDER: &str = "Such as Status or Health";

/// Accessible name of the property's name.
pub const PROPERTY_NAME_FIELD: &str = "Property name";

/// Kind of a property that holds one of a few named states.
pub const STATES_KIND: &str = "States";

/// Kind of a property that holds a number.
pub const NUMBER_KIND: &str = "Number";

/// Description of [`STATES_KIND`].
pub const STATES_KIND_DESCRIPTION: &str = "One of a few named states, such as Dead and Alive. Give it at least two.";

/// Description of [`NUMBER_KIND`].
pub const NUMBER_KIND_DESCRIPTION: &str = "Holds a number, such as an age or an amount of food.";

/// Kind of a Number property that holds whole numbers only.
pub const INTEGER_KIND: &str = "Integer";

/// Tooltip of [`INTEGER_KIND`].
pub const INTEGER_KIND_TOOLTIP: &str = "A number with no fraction, such as 3 or -2.";

/// Button that adds a state to the form.
pub const ADD_STATE: &str = "+ Add state";

/// Returns the accessible name of the colour button of state `number`, such as "State 1 colour: Dark grey".
pub fn state_color_name(number: usize, color: &str) -> String {
    format!("State {number} colour: {color}")
}

/// Returns the accessible name of the name field of state `number`, such as "State 1 name".
pub fn state_name_field(number: usize) -> String {
    format!("State {number} name")
}

/// Returns the accessible name of the button that removes state `number`, such as "Remove state 1".
pub fn remove_state_name(number: usize) -> String {
    format!("Remove state {number}")
}

/// Message on a property with no name.
pub const PROPERTY_NAME_MISSING: &str = "Type a name for the property.";

/// Returns the message on a property named `name`, which another property holds.
pub fn property_name_taken(name: &str) -> String {
    format!("A property called {name} already exists.")
}

/// Message on a States property with fewer than two states.
pub const TOO_FEW_STATES: &str = "Give the property at least two states.";

/// Message on a States property with two states of one name.
pub const STATE_NAMES_REPEATED: &str = "Two states have the same name. Give each state its own name.";

/// Returns the heading of the form that edits the property `property`, such as "Edit property Status".
pub fn edit_property_heading(property: &str) -> String {
    format!("Edit property {property}")
}

/// Button that deletes the property being edited.
pub const DELETE_PROPERTY: &str = "Delete property";

// Starts as

/// Returns the choice of Starts as where every cell starts in `state`, such as "All Dead".
pub fn starts_all(state: &str) -> String {
    format!("All {state}")
}

/// Choice of Starts as where a state is drawn by chance.
pub const BY_CHANCE: &str = "By chance";

/// Words between the state and the chance of a row of Starts as.
pub const WITH_CHANCE: &str = "with chance";

/// Label of the button that removes a row of Starts as.
pub const REMOVE_ROW_LABEL: &str = "×";

/// Button that adds a row to Starts as.
pub const ADD_ROW: &str = "+ Add row";

/// Returns the accessible name of [`ADD_ROW`] for `property`, such as "Add row to Status starts as".
pub fn add_row_name(property: &str) -> String {
    format!("Add row to {property} starts as")
}

/// Returns the accessible name of the button that removes row `row`, such as "Remove row 1 of Status starts as".
pub fn remove_row_name(row: usize, property: &str) -> String {
    format!("Remove row {row} of {property} starts as")
}

/// Returns the accessible name of the state of the last row of Starts as, such as "Status starts as, otherwise".
pub fn starts_otherwise_name(property: &str) -> String {
    format!("{property} starts as, {OTHERWISE}")
}

/// Choice of Starts as where every cell starts at one number.
pub const A_VALUE: &str = "A value";

/// Words before the number of [`A_VALUE`].
pub const STARTS_AT: &str = "Every cell starts at";

/// Choice of Starts as where every cell draws a number.
pub const A_RANDOM_NUMBER: &str = "A random number";

/// Words before the low end of [`A_RANDOM_NUMBER`].
pub const RANDOM_NUMBER_BETWEEN: &str = "A random number between";

/// Word between the low end and the high end of [`A_RANDOM_NUMBER`].
pub const RANDOM_NUMBER_AND: &str = "and";

/// One control of Starts as that takes an accessible name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartsField {
    /// The state of a row.
    State,
    /// The chance of a row.
    Chance,
    /// A number.
    Number,
    /// The "Use a parameter" button beside a field.
    UseParameter,
}

impl StartsField {
    /// Word of the control after its place, such as "chance".
    pub fn word(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Chance => "chance",
            Self::Number => "number",
            Self::UseParameter => "use a parameter",
        }
    }
}

/// Returns the place of row `row` of the Starts as of `property`, such as "Status starts as, row 1".
pub fn starts_row_place(property: &str, row: usize) -> String {
    format!("{property} starts as, row {row}")
}

/// Returns the place of the one number of the Starts as of `property`, such as "Food starts as".
pub fn starts_place(property: &str) -> String {
    format!("{property} starts as")
}

/// Returns the place of the low end of the Starts as of `property`, such as "Food starts as, low end".
pub fn starts_low_place(property: &str) -> String {
    format!("{property} starts as, low end")
}

/// Returns the place of the high end of the Starts as of `property`, such as "Food starts as, high end".
pub fn starts_high_place(property: &str) -> String {
    format!("{property} starts as, high end")
}

/// Returns the accessible name of a control of Starts as at `place`, such as "Status starts as, row 1: chance".
pub fn starts_field_name(place: &str, field: StartsField) -> String {
    format!("{place}: {}", field.word())
}

/// Message under a chance of Starts as that does not read.
pub const TYPE_A_CHANCE: &str = "Type a chance such as 0.3 or 30%.";

/// Button beside a field of Starts as that opens [`CHOOSE_A_PARAMETER`].
pub const USE_A_PARAMETER: &str = "Use a parameter";

/// Title of the picker of parameters for a field of Starts as.
pub const CHOOSE_A_PARAMETER: &str = "Choose a parameter";

/// Note on Starts as once the world is past its first tick.
pub const START_AGAIN_TO_SEE: &str = "Start again to see it";

// Parameters on the sheet

/// Columns of the Parameters table.
pub const PARAMETER_COLUMNS: [&str; 5] = [NAME_LABEL, "Value", MIN_LABEL, MAX_LABEL, UNIT_LABEL];

/// Returns the accessible name of the name field of the parameter `parameter`, such as "Name of Infection rate".
pub fn sheet_parameter_name_field(parameter: &str) -> String {
    format!("Name of {parameter}")
}

/// Returns the accessible name of the slider of `parameter`, such as "Infection rate value slider".
pub fn value_slider_name(parameter: &str) -> String {
    format!("{parameter} value slider")
}

/// Returns the accessible name of the value field of `parameter`, such as "Infection rate value".
pub fn value_field_name(parameter: &str) -> String {
    format!("{parameter} value")
}

/// Returns the accessible name of the button that deletes `parameter`, such as "Delete parameter Infection rate".
pub fn delete_parameter_name(parameter: &str) -> String {
    format!("Delete parameter {parameter}")
}

/// Returns the accessible name of the minimum of `parameter`, such as "Infection rate min".
pub fn parameter_min_name(parameter: &str) -> String {
    format!("{parameter} min")
}

/// Returns the accessible name of the maximum of `parameter`, such as "Infection rate max".
pub fn parameter_max_name(parameter: &str) -> String {
    format!("{parameter} max")
}

/// Returns the accessible name of the unit of `parameter`, such as "Infection rate unit".
pub fn parameter_unit_name(parameter: &str) -> String {
    format!("{parameter} unit")
}

/// Placeholder of a parameter's unit on the sheet.
pub const UNIT_FIELD_PLACEHOLDER: &str = "none";

/// Returns the suggestion for a parameter's unit in `unit`, such as "per day".
pub fn unit_suggestion(unit: TimeUnit) -> String {
    format!("per {}", unit.as_str())
}

/// Message on a parameter of the sheet with no name.
pub const PARAMETER_NAME_MISSING: &str = "A parameter needs a name.";

/// Returns the message on a parameter named `name`, which another parameter holds.
pub fn parameter_name_taken(name: &str) -> String {
    format!("A parameter called {name} already exists.")
}

/// Returns the line of the sheet while the world tries the value `tried` and the model keeps `kept`.
pub fn sheet_trying_value(tried: &str, kept: &str) -> String {
    format!("The world is trying {tried}. The model keeps {kept}.")
}

/// Button that adds a parameter on the sheet.
pub const ADD_PARAMETER: &str = "+ Add parameter";

// Measures

/// Returns the label of the box that plots the number of cells in `state`, such as "Plot Alive".
pub fn plot_state(state: &str) -> String {
    format!("Plot {state}")
}

/// Button that opens the measure form.
pub const ADD_MEASURE: &str = "+ Add measure";

/// Heading of the measure form.
pub const NEW_MEASURE: &str = "New measure";

/// Kind of a measure that counts the cells in a state.
pub const COUNT_MEASURE: &str = "Number of cells in a state";

/// Returns the words of a measure that counts the cells whose `property` is `state`, such as "Number of cells whose
/// Status is Alive".
pub fn count_measure(property: &str, state: &str) -> String {
    format!("Number of cells whose {property} is {state}")
}

/// Kind of a measure that averages a number property.
pub const AVERAGE_MEASURE: &str = "Average of a number";

/// Returns the words of a measure that averages `property`, such as "Average of Food".
pub fn average_measure(property: &str) -> String {
    format!("Average of {property}")
}

/// Heading of the list of measures.
pub const PLOTTED: &str = "Plotted";

/// Returns the accessible name of the colour button of the measure `measure`, such as "Colour of Alive: Blue".
pub fn measure_color_name(measure: &str, color: &str) -> String {
    format!("Colour of {measure}: {color}")
}

/// Returns the accessible name of the button that removes `measure`, such as "Remove measure Alive".
pub fn remove_measure_name(measure: &str) -> String {
    format!("Remove measure {measure}")
}

/// Message on a count measure with no property or no state.
pub const MEASURE_NEEDS_STATE: &str = "Choose a property and a state.";

/// Message on an average measure with no property.
pub const MEASURE_NEEDS_NUMBER_PROPERTY: &str = "Choose a number property.";

/// Message on a measure that is already on the plot.
pub const MEASURE_ALREADY_PLOTTED: &str = "That measure is already plotted.";

// Colours

/// Words before the property that colours the world.
pub const COLOR_CELLS_BY: &str = "Colour cells by";

/// Label of the shade of a Number property.
pub const SHADE: &str = "Shade";

/// Shades of a Number property, in the order the select shows them.
pub const SHADE_COLORS: [&str; 7] = ["Grey", "Red", "Orange", "Yellow", "Green", "Blue", "Purple"];

/// Returns the range of a shade, such as "from 0 (dark) to 10 (light)".
pub fn shade_range(darkest: &str, lightest: &str) -> String {
    format!("from {darkest} (dark) to {lightest} (light)")
}

/// Accessible name of the value drawn darkest.
pub const DARKEST_NAME: &str = "Darkest at value";

/// Accessible name of the value drawn lightest.
pub const LIGHTEST_NAME: &str = "Lightest at value";

// Buttons on the sheet

/// Button that opens the button form.
pub const ADD_BUTTON: &str = "+ Add button";

/// Placeholder of a button's name.
pub const BUTTON_NAME_PLACEHOLDER: &str = "Such as Clear";

/// Accessible name of a button's name.
pub const BUTTON_NAME_FIELD: &str = "Button name";

/// Label of what a button does.
pub const BUTTON_DOES: &str = "Does";

/// What a button does that redraws how cells start, and the world's button of the same name.
pub const START_AGAIN: &str = "Start again";

/// Choice of [`START_AGAIN`] that redraws every property.
pub const EVERY_PROPERTY: &str = "every property";

/// Accessible name of the property [`START_AGAIN`] redraws.
pub const START_AGAIN_FOR: &str = "Start again for";

/// Returns what a button does that runs its own lines once over `thing`, such as "Run once, every cell".
pub fn run_once(thing: &str) -> String {
    format!("Run once, every {thing}")
}

/// Returns the accessible name of the button that edits the button `button`, such as "Edit button Clear".
pub fn edit_button_name(button: &str) -> String {
    format!("Edit button {button}")
}

/// Returns the accessible name of the button that deletes the button `button`, such as "Delete button Clear".
pub fn delete_button_name(button: &str) -> String {
    format!("Delete button {button}")
}

/// Message on a button with no name.
pub const BUTTON_NAME_MISSING: &str = "Type a name for the button.";

/// Returns the message on a button named `name`, which another button holds.
pub fn button_name_taken(name: &str) -> String {
    format!("A button called {name} already exists.")
}

// The New model dialog

/// Heading of the New model dialog, and the top bar's button that opens it.
pub const NEW_MODEL: &str = "New model";

/// Label of the starters.
pub const START_FROM: &str = "Start from";

/// Starters of a cells model with their descriptions, in the order the dialog shows them.
pub const STARTER_CHOICES: [(&str, &str); 5] = [
    ("Blank", "An empty grid of cells to build on."),
    (
        "Game of Life",
        "Conway's Life. A dead cell with 3 live neighbours comes alive.",
    ),
    (
        "SIR epidemic",
        "An epidemic that spreads to neighbours at a rate per day.",
    ),
    ("Majority vote", "Each cell takes the opinion of most of its block."),
    (
        "Forest fire",
        "Trees grow, lightning strikes, and fire spreads to neighbours.",
    ),
];

/// Question under Blank only.
pub const WORLD_QUESTION: &str = "What is the world made of?";

/// Answer to [`WORLD_QUESTION`] beside [`CELLS_ON_A_GRID`].
pub const AGENTS_IN_OPEN_SPACE: &str = "Agents in open space";

/// Name of a new model before the author types one.
pub const UNTITLED_MODEL: &str = "Untitled model";

/// Primary action of the New model dialog.
pub const CREATE: &str = "Create";

// The New parameter dialog

/// Heading of the New parameter dialog.
pub const NEW_PARAMETER_HEADING: &str = "New parameter";

/// Lead of the New parameter dialog.
pub const NEW_PARAMETER_LEAD: &str = "A named number with a slider. Lines that use it change as the slider moves.";

/// Accessible name of the parameter's name.
pub const PARAMETER_NAME_FIELD: &str = "Parameter name";

/// Placeholder of the parameter's name when it is for a rate.
pub const RATE_PARAMETER_PLACEHOLDER: &str = "Such as Infection rate";

/// Placeholder of the parameter's name when it is for a chance.
pub const CHANCE_PARAMETER_PLACEHOLDER: &str = "Such as Initial density";

/// Placeholder of the parameter's name when it is for any other number.
pub const NUMBER_PARAMETER_PLACEHOLDER: &str = "Such as Step size";

/// Label of the parameter's default.
pub const DEFAULT_LABEL: &str = "Default";

/// Accessible name of the parameter's default.
pub const DEFAULT_NAME: &str = "Default value";

/// Label of a parameter's minimum.
pub const MIN_LABEL: &str = "Min";

/// Label of a parameter's maximum.
pub const MAX_LABEL: &str = "Max";

/// Label of a parameter's unit.
pub const UNIT_LABEL: &str = "Unit";

/// Placeholder of the parameter's unit in the dialog.
pub const DIALOG_UNIT_PLACEHOLDER: &str = "Optional, such as per day";

/// Message on a new parameter with no name.
pub const DIALOG_NAME_MISSING: &str = "Type a name.";

/// Message on a default, a minimum or a maximum that does not read.
pub const LIMITS_NOT_NUMBERS: &str = "Default, Min and Max must be numbers.";

/// Message on a minimum at or above the maximum.
pub const MIN_NOT_BELOW_MAX: &str = "Min must be below Max.";

/// Message on a default outside the minimum and the maximum.
pub const DEFAULT_OUTSIDE_LIMITS: &str = "Default must be between Min and Max.";

/// Message on a parameter for a chance whose minimum or maximum lies outside 0 to 1.
pub const CHANCE_LIMITS_OUTSIDE: &str = "A chance stays between 0 and 1. Set Min and Max inside that range.";

/// Primary action of the New parameter dialog.
pub const CREATE_PARAMETER: &str = "Create parameter";

// The top bar

/// Returns the accessible name of the button that shows the model's name `name`, such as "Model name: Game of Life.
/// Rename".
pub fn model_name_button(name: &str) -> String {
    format!("Model name: {name}. Rename")
}

/// Accessible name of the field that renames the model.
pub const MODEL_NAME_FIELD: &str = "Model name";

/// Button that undoes the last edit.
pub const UNDO: &str = "Undo";

/// Button that redoes the last undone edit.
pub const REDO: &str = "Redo";

/// Toggle that draws the card as a diagram.
pub const DIAGRAM: &str = "Diagram";

/// Tooltip of [`DIAGRAM`].
pub const DIAGRAM_TOOLTIP: &str = "Draw the lines as boxes and arrows";

// The world tabs

/// Returns the tick counter at tick `tick`: "Tick 4 (a tick is one step of time)", or "Tick 4 (4 days)" when the ticks
/// so far last `elapsed`.
pub fn tick_counter(tick: u64, elapsed: Option<&str>) -> String {
    format!("Tick {tick} ({})", elapsed.unwrap_or("a tick is one step of time"))
}

/// Returns the note beside a parameter's slider while the world tries a value and the model keeps `kept`.
pub fn trying_value(kept: &str) -> String {
    format!("Trying this value. The model keeps {kept}.")
}

/// Button that returns a tried parameter to the value the model keeps.
pub const RESET: &str = "Reset";

/// Returns the accessible name of [`RESET`] for `parameter`, such as "Reset Lightning f to 0.0001".
pub fn reset_name(parameter: &str, kept: &str) -> String {
    format!("Reset {parameter} to {kept}")
}

/// Button that makes a tried value the one the model keeps.
pub const KEEP: &str = "Keep";

/// Returns the accessible name of [`KEEP`], such as "Keep 0.0005 for Lightning f".
pub fn keep_name(tried: &str, parameter: &str) -> String {
    format!("Keep {tried} for {parameter}")
}

/// Tooltip of [`KEEP`].
pub const KEEP_TOOLTIP: &str = "Make it the value the model keeps";

/// Text of the plot while the sheet holds no measure.
pub const PLOT_EMPTY: &str = "Tick Plot beside a state under Measures to plot the number of cells in it.";

/// Text of the world while the sheet holds no property.
pub const WORLD_EMPTY: &str = "Add a property on the sheet to see the world.";

// The Diagram

/// Note above the Diagram.
pub const DIAGRAM_NOTE: &str = "Read-only. Edit the lines to change it.";

/// Returns the accessible name of the Diagram of a card holding `count` lines, such as "Diagram of the card's 3
/// lines, read-only".
pub fn diagram_name(count: usize) -> String {
    format!("Diagram of the card's {}, read-only", line_count(count))
}

/// Returns the label of the frame around the lines inside line `number` in the Diagram, such as "Inside line 4".
pub fn inside_line_frame(number: usize) -> String {
    format!("Inside line {number}")
}

// Conversion to a node graph. These words sit under the exemption for the controls that open a node graph, and
// `FORBIDDEN_WORDS` does not cover them.

/// Tooltip of the disabled Convert while the card holds an error.
pub const CONVERT_NEEDS_FIXES: &str = "Fix the problems on the card to convert.";

/// Tooltip of the disabled Convert while the card holds a line that does not run.
pub const CONVERT_NEEDS_FINISHED_LINES: &str = "Some lines do not run yet. Finish or delete them to convert.";

/// Heading of the conversion dialog.
pub const CONVERT_HEADING: &str = "Convert to node graph";

/// Returns the name of the copy a conversion makes of the model `name`, such as "Game of Life (expert)".
pub fn expert_copy_name(name: &str) -> String {
    format!("{name} (expert)")
}

/// Returns the two paragraphs of the conversion dialog for the model `name`, each sentence on a line of its own.
pub fn convert_text(name: &str) -> [String; 2] {
    [
        format!(
            "{name} will be copied to a new model, {}, as a node graph.\n{name} stays as it is.",
            expert_copy_name(name)
        ),
        "In the node graph you can edit every node Henad runs.\nThe copy has no sheet or cards, so lines, fire counts \
         and \"otherwise\" will not appear in it.\nChanges to the copy cannot be turned back into lines."
            .to_owned(),
    ]
}

/// Primary action of the conversion dialog.
pub const CONVERT: &str = "Convert";

// Lists

/// Returns line numbers as a list ending in "and": "2", "2 and 5", "2, 3 and 4".
pub fn number_list(numbers: &[usize]) -> String {
    let words: Vec<String> = numbers.iter().map(usize::to_string).collect();
    word_list(&words)
}

/// Returns `words` as a list ending in "and": "Dead and Alive", "Susceptible, Infected and Recovered".
pub fn word_list(words: &[String]) -> String {
    match words.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// Returns "an" before a word starting with a vowel, and "a" otherwise.
fn indefinite_article(word: &str) -> &'static str {
    match word.chars().next().map(|first| first.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

/// Returns `words` with its first letter in lower case when it starts an ordinary capitalised word, a capital and a
/// small letter. A one-letter or all-capital word keeps its capital.
fn drop_ordinary_capital(words: &str) -> String {
    let mut chars = words.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if first.is_uppercase() && second.is_lowercase() => {
            let mut out: String = first.to_lowercase().collect();
            out.push_str(&words[first.len_utf8()..]);
            out
        }
        _ => words.to_owned(),
    }
}

/// Returns `text` with its first letter in lower case, as a change reads after "otherwise".
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Returns `text` with its first letter in upper case, as a line's sentence starts.
fn upper_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Returns `name` in the possessive: "Flockmates'" or "Next cell's".
fn possessive(name: &str) -> String {
    if name.ends_with('s') {
        format!("{name}'")
    } else {
        format!("{name}'s")
    }
}

/// Returns `name` in the plural: "birds", or `name` itself when it ends in "s".
pub(crate) fn plural(name: &str) -> String {
    if name.ends_with('s') {
        name.to_owned()
    } else {
        format!("{name}s")
    }
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
        match every {
            "cells" => "cell".to_owned(),
            "ground" => "ground cell".to_owned(),
            _ => self
                .document
                .agents
                .iter()
                .find(|kind| kind.id == every)
                .map_or_else(|| every.to_owned(), |kind| drop_ordinary_capital(&kind.name)),
        }
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

    /// Returns the sentence of line `line_id`, where `otherwise` holds the numbers of the higher lines that override
    /// it, empty for a line that no line overrides.
    ///
    /// A line that no card holds reads as [`BLANK_TEXT`].
    pub fn line(&self, line_id: &str, otherwise: &[usize]) -> String {
        let Some(line) = self.lines.get(line_id) else {
            return BLANK_TEXT.to_owned();
        };
        if is_result_kind(line.kind) {
            let phrase = self.phrase(line, false);
            return match &line.name {
                Some(name) if !(line.kind == LineKind::Compare && *name == phrase) => format!("{name} = {phrase}"),
                _ => phrase,
            };
        }
        let text = match line.kind {
            LineKind::When => return format!("{}:", self.condition(&line.condition, true)),
            LineKind::Repeat => return format!("Repeat {} times:", self.slot(&line.times, None)),
            LineKind::Always => return ALWAYS.to_owned(),
            _ => self.change(line),
        };
        if otherwise.is_empty() {
            upper_first(&text)
        } else {
            format!("{} {}", otherwise_words(otherwise), lower_first(&text))
        }
    }

    /// Returns the words of the result line or phrase `line` after its name, such as "Alive neighbours is 3".
    pub fn result_words(&self, line: &SheetLine) -> String {
        self.phrase(line, false)
    }

    /// Returns the words of a change line with its condition, with the first letter in upper case.
    fn change(&self, line: &SheetLine) -> String {
        let property = self.property_name(line.property.as_deref());
        let body = match line.kind {
            LineKind::Become => {
                let new = self.state_name(line.property.as_deref(), line.new_state.as_deref());
                let old = match line.start_state.as_deref() {
                    Some(ANY_STATE_ID) => ANY_STATE.to_owned(),
                    other => self.state_name(line.property.as_deref(), other),
                };
                format!("Become {new} from {old}")
            }
            LineKind::Set => format!("Set {property} to {}", self.slot(&line.value, None)),
            LineKind::Steer => format!(
                "Steer {property} {} {}, strength {}",
                mode_words(line.mode),
                self.slot(&line.value, None),
                self.slot(&line.strength, None)
            ),
            LineKind::KeepBetween => {
                let arrow = line
                    .property
                    .as_deref()
                    .and_then(|id| self.properties.get(id))
                    .is_some_and(|property| property.property_type == PropertyType::Arrow);
                let measured = if arrow {
                    format!("{} length", possessive(&property))
                } else {
                    property
                };
                format!(
                    "Keep {measured} between {} and {}",
                    self.slot(&line.minimum, None),
                    self.slot(&line.maximum, None)
                )
            }
            LineKind::Move => format!("Move {} {}", mode_words(line.mode), self.slot(&line.value, None)),
            LineKind::Leave => format!(
                "Leave {} on {}",
                self.slot(&line.amount, None),
                self.property_name(line.ground_property.as_deref())
            ),
            LineKind::Record => format!(
                "Record a {}",
                line.event.as_deref().map_or(BLANK_TEXT, |id| self
                    .events
                    .get(id)
                    .map_or(id, |event| event.name.as_str()))
            ),
            _ => line.kind.as_str().to_owned(),
        };
        let takes_condition = matches!(
            line.kind,
            LineKind::Become | LineKind::Set | LineKind::Leave | LineKind::Record
        );
        if takes_condition || !line.condition.is_blank() {
            format!("{body}, {}", self.condition(&line.condition, false))
        } else {
            body
        }
    }

    /// Returns the condition in `slot`: "always", "when ...", "with chance ..." or "at rate ...", or on a When line
    /// "When ...", "When with chance ..." or "When at rate ...".
    fn condition(&self, slot: &Slot, on_when_line: bool) -> String {
        let blank_head = if on_when_line { "When " } else { "" };
        let Some(value) = slot.value() else {
            return format!("{blank_head}{BLANK_TEXT}");
        };
        if let SheetValue::Phrase(phrase) = value {
            match phrase.kind {
                LineKind::Always if on_when_line => return format!("When {ALWAYS}"),
                LineKind::Always => return ALWAYS.to_owned(),
                LineKind::Chance => {
                    let head = condition_head(ConditionKind::Chance, on_when_line);
                    return format!("{head} {}", self.chance_body(phrase));
                }
                LineKind::Rate => {
                    let head = condition_head(ConditionKind::Rate, on_when_line);
                    return format!("{head} {}", self.rate_body(phrase));
                }
                _ => {}
            }
        }
        let head = condition_head(ConditionKind::When, on_when_line);
        format!("{head} {}", self.value(value, None))
    }

    /// Returns the words of the value in `slot`, [`BLANK_TEXT`] for a blank slot.
    ///
    /// `about` is the property that a state in the slot belongs to.
    pub fn slot(&self, slot: &Slot, about: Option<&str>) -> String {
        slot.value()
            .map_or_else(|| BLANK_TEXT.to_owned(), |value| self.value(value, about))
    }

    /// Returns the words of `value`, where `about` is the property that a state in it belongs to.
    pub fn value(&self, value: &SheetValue, about: Option<&str>) -> String {
        match value {
            SheetValue::Literal(literal) => literal_words(*literal),
            SheetValue::Chip(chip) => self.chip(chip, about),
            SheetValue::Phrase(phrase) if phrase.kind == LineKind::Calculate => {
                format!("({})", self.phrase(phrase, true))
            }
            SheetValue::Phrase(phrase) => self.phrase(phrase, true),
            SheetValue::List(items) => {
                let words: Vec<String> = items.iter().map(|item| self.value(item, about)).collect();
                words.join(", ")
            }
        }
    }

    /// Returns the words of `chip`, where `about` is the property that a state chip belongs to.
    fn chip(&self, chip: &SheetChip, about: Option<&str>) -> String {
        match chip {
            SheetChip::Param(id) => self
                .parameters
                .get(id.as_str())
                .map_or_else(|| id.clone(), |parameter| parameter.name.clone()),
            SheetChip::My(id) => format!("my {}", self.property_name(Some(id))),
            SheetChip::MyNew(id) => format!("my new {}", self.property_name(Some(id))),
            SheetChip::Their(id) => format!("their {}", self.property_name(Some(id))),
            SheetChip::Relation(relation) => relation_words(*relation).to_owned(),
            SheetChip::State(id) => self.state_name(about, Some(id)),
            SheetChip::Result { line, part, property } => {
                let result = self.lines.get(line.as_str());
                let base = result
                    .and_then(|result| result.name.clone())
                    .unwrap_or_else(|| line.clone());
                if let Some(part) = part {
                    if part == "open" {
                        return format!("{base} is open");
                    }
                    let part = result
                        .and_then(|result| result.summaries.iter().find(|summary| summary.id == *part))
                        .map_or(part.as_str(), |summary| summary.name.as_str());
                    return format!("{} {part}", possessive(&base));
                }
                match property {
                    Some(property) => format!("{} {}", possessive(&base), self.property_name(Some(property))),
                    None => base,
                }
            }
        }
    }

    /// Returns the words of the phrase or result line `line` after its name.
    ///
    /// `inline` is whether the phrase sits inside a slot, where an all of reads as its items joined by "and".
    #[expect(clippy::too_many_lines, reason = "one arm per line kind reads best as one table")]
    fn phrase(&self, line: &SheetLine, inline: bool) -> String {
        match line.kind {
            LineKind::Count => {
                let mut text = format!("count of {}", self.around(line));
                if !line.whose.is_blank() {
                    let whose = self.slot(&line.whose, None);
                    text.push_str(" whose ");
                    text.push_str(whose.strip_prefix("their ").unwrap_or(&whose));
                }
                text
            }
            LineKind::LookAround => {
                // A Total or average line of a cells card is a Look around over cells with one summary.
                if let (Some(cells), [summary]) = (line.around.and_then(around_words), line.summaries.as_slice())
                    && line.only.is_blank()
                    && let Some(kind @ (SummaryKind::Total | SummaryKind::Average)) = summary.summary
                {
                    let value = self.slot(&summary.value, None);
                    let property = value.strip_prefix("their ").unwrap_or(&value);
                    return format!("{} of {property} over {cells}", summary_word(kind));
                }
                let mut text = format!("look at {}", self.around(line));
                if !line.only.is_blank() {
                    let only = self.slot(&line.only, None);
                    text.push_str(", only those whose ");
                    text.push_str(only.strip_prefix("their ").unwrap_or(&only));
                }
                let summaries: Vec<String> = line
                    .summaries
                    .iter()
                    .map(|summary| match summary.summary {
                        Some(SummaryKind::Count) => format!("count ({})", summary.name),
                        kind => format!(
                            "{} of {} ({})",
                            kind.map_or(BLANK_TEXT, summary_word),
                            self.slot(&summary.value, None),
                            summary.name
                        ),
                    })
                    .collect();
                format!("{text}: {}", summaries.join(", "))
            }
            LineKind::PickCell => {
                let open = if line.open_only == Some(true) { "open " } else { "" };
                match line.pick {
                    Some(PickRule::Random) => format!("a random {open}neighbouring cell"),
                    pick => format!(
                        "the {open}neighbouring cell with the {} {} (ties at random)",
                        pick.map_or(BLANK_TEXT, |pick| pick.as_str()),
                        self.property_name(line.ranked_by.as_deref())
                    ),
                }
            }
            LineKind::LookAtCell => match line.which {
                Some(CellWhich::Here) => "the cell I am on".to_owned(),
                Some(CellWhich::Ahead) => "the cell ahead of me".to_owned(),
                Some(CellWhich::At) => format!("the cell at {}", self.slot(&line.place, None)),
                None => BLANK_TEXT.to_owned(),
            },
            LineKind::Chance => format!(
                "{} {}",
                condition_head(ConditionKind::Chance, false),
                self.chance_body(line)
            ),
            LineKind::Rate => format!(
                "{} {}",
                condition_head(ConditionKind::Rate, false),
                self.rate_body(line)
            ),
            LineKind::RandomNumber => format!(
                "a random number between {} and {}",
                self.slot(&line.low, None),
                self.slot(&line.high, None)
            ),
            LineKind::RandomDirection => format!("a random direction of length {}", self.slot(&line.length, None)),
            LineKind::Compare => self.compare(line),
            LineKind::AllOf if inline => self.inline_all_of(line),
            LineKind::AllOf | LineKind::AnyOf | LineKind::NoneOf => {
                let words = match line.kind {
                    LineKind::AllOf => "all of",
                    LineKind::AnyOf => "any of",
                    _ => "none of",
                };
                let items: Vec<String> = line.items.iter().map(|item| self.value(item, None)).collect();
                if items.is_empty() {
                    format!("{words} {BLANK_TEXT}")
                } else {
                    format!("{words} {}", items.join(", "))
                }
            }
            LineKind::Choose => {
                let mut rows: Vec<String> = line
                    .rows
                    .iter()
                    .map(|row| {
                        format!(
                            "{}, {}",
                            self.slot(&row.value, None),
                            self.condition(&row.condition, false)
                        )
                    })
                    .collect();
                rows.push(format!("otherwise {}", self.slot(&line.otherwise, None)));
                format!("choose, first that applies: {}", rows.join("; "))
            }
            LineKind::DependingOn => {
                let property = line.property.as_deref();
                let states: Vec<&str> = property
                    .and_then(|id| self.properties.get(id))
                    .map(|property| property.states.iter().map(|state| state.id.as_str()).collect())
                    .unwrap_or_default();
                let ordered = states
                    .iter()
                    .copied()
                    .filter(|state| line.branches.contains_key(*state))
                    .chain(
                        line.branches
                            .keys()
                            .map(String::as_str)
                            .filter(|state| !states.contains(state)),
                    );
                let branches: Vec<String> = ordered
                    .filter_map(|state| {
                        line.branches.get(state).map(|value| {
                            format!(
                                "{} -> {}",
                                self.state_name(property, Some(state)),
                                self.value(value, None)
                            )
                        })
                    })
                    .collect();
                format!(
                    "depending on my {}: {}",
                    self.property_name(property),
                    branches.join(", ")
                )
            }
            LineKind::Calculate => self.formula(line.formula.as_deref().unwrap_or(BLANK_TEXT)),
            LineKind::Always => ALWAYS.to_owned(),
            _ => self.change(line),
        }
    }

    /// Returns the words of a chance after "with chance": its chance and its "for each".
    fn chance_body(&self, line: &SheetLine) -> String {
        format!("{}{}", self.slot(&line.chance, None), self.for_each(&line.tries))
    }

    /// Returns the words of a rate after "at rate": its rate, its unit and its "for each".
    fn rate_body(&self, line: &SheetLine) -> String {
        format!(
            "{} per {}{}",
            self.slot(&line.rate, None),
            line.rate_unit.map_or(BLANK_TEXT, rate_unit_word),
            self.for_each(&line.tries)
        )
    }

    /// Returns " for each ..." for a filled "for each" slot, and nothing for a blank one.
    ///
    /// A named result reads in the singular through [`for_each_words`].
    fn for_each(&self, tries: &Slot) -> String {
        match tries.value() {
            None => String::new(),
            Some(SheetValue::Chip(SheetChip::Result {
                line,
                part: None,
                property: None,
            })) => match self.lines.get(line.as_str()).and_then(|line| line.name.as_deref()) {
                Some(name) => format!(" for each {}", for_each_words(name)),
                None => format!(" for each {line}"),
            },
            Some(value) => format!(" for each {}", self.value(value, None)),
        }
    }

    /// Returns the words of a comparison.
    fn compare(&self, line: &SheetLine) -> String {
        let about = Self::compared_property(&line.left);
        let left = self.slot(&line.left, None);
        let Some(test) = line.test else {
            return format!("{left} {BLANK_TEXT} {}", self.slot(&line.right, about));
        };
        match test {
            CompareTest::Within => format!(
                "{left} is within {} of {}",
                self.slot(&line.tolerance, None),
                self.slot(&line.right, about)
            ),
            CompareTest::Outside => outside_words(&left, &self.slot(&line.low, None), &self.slot(&line.high, None)),
            _ => {
                let of_states = about
                    .and_then(|id| self.properties.get(id))
                    .is_some_and(|property| property.property_type == PropertyType::States);
                format!(
                    "{left} {} {}",
                    comparison_words(test, of_states),
                    self.slot(&line.right, about)
                )
            }
        }
    }

    /// Returns the property that the value in `slot` reads, `None` when it reads none.
    fn compared_property(slot: &Slot) -> Option<&str> {
        match slot.value()? {
            SheetValue::Chip(SheetChip::My(id) | SheetChip::MyNew(id) | SheetChip::Their(id)) => Some(id),
            SheetValue::Chip(SheetChip::Result {
                property: Some(property),
                ..
            }) => Some(property),
            _ => None,
        }
    }

    /// Returns the words of an all of inside a slot: its items joined by "and", and its draws after commas.
    fn inline_all_of(&self, line: &SheetLine) -> String {
        let is_draw = |item: &SheetValue| match item {
            SheetValue::Phrase(phrase) => matches!(phrase.kind, LineKind::Chance | LineKind::Rate),
            _ => false,
        };
        let tests: Vec<String> = line
            .items
            .iter()
            .filter(|item| !is_draw(item))
            .map(|item| self.value(item, None))
            .collect();
        let mut parts: Vec<String> = Vec::new();
        if !tests.is_empty() {
            parts.push(tests.join(" and "));
        }
        parts.extend(
            line.items
                .iter()
                .filter(|item| is_draw(item))
                .map(|item| self.value(item, None)),
        );
        if parts.is_empty() {
            format!("all of {BLANK_TEXT}")
        } else {
            parts.join(", ")
        }
    }

    /// Returns the words of the neighbourhood a Count or Look around line looks at.
    fn around(&self, line: &SheetLine) -> String {
        match line.around {
            Some(Around::AgentsWithin) => {
                let thing = line
                    .agent_kind
                    .as_deref()
                    .map_or_else(|| BLANK_TEXT.to_owned(), |kind| plural(&self.thing_word(kind)));
                let mut text = format!("{thing} within {}", self.slot(&line.distance, None));
                if line.include_me == Some(true) {
                    text.push_str(" including me");
                }
                text
            }
            Some(around) => around_words(around).unwrap_or(BLANK_TEXT).to_owned(),
            None => BLANK_TEXT.to_owned(),
        }
    }

    /// Returns the words of `formula`: its references by name, its operators as words, and the parentheses of a call
    /// left out.
    fn formula(&self, formula: &str) -> String {
        let mut words: Vec<String> = Vec::new();
        // Whether each open parenthesis is a call's. The words leave a call's parentheses out.
        let mut calls: Vec<bool> = Vec::new();
        let mut glue_next = false;
        let tokens = formula_tokens(formula);
        for (index, token) in tokens.iter().enumerate() {
            let previous = index.checked_sub(1).and_then(|before| tokens.get(before));
            let word = match token.as_str() {
                "(" => {
                    let call = previous.is_some_and(|previous| function_words(previous).is_some());
                    calls.push(call);
                    if call {
                        continue;
                    }
                    words.push("(".to_owned());
                    glue_next = true;
                    continue;
                }
                ")" => {
                    if !calls.pop().unwrap_or(false) {
                        match words.last_mut() {
                            Some(last) => last.push(')'),
                            None => words.push(")".to_owned()),
                        }
                    }
                    continue;
                }
                "," => "and".to_owned(),
                "-" if previous.is_none_or(|previous| {
                    operation_words(previous).is_some() || matches!(previous.as_str(), "(" | ",")
                }) =>
                {
                    words.push("-".to_owned());
                    glue_next = true;
                    continue;
                }
                operator if operation_words(operator).is_some() => {
                    operation_words(operator).unwrap_or_default().to_owned()
                }
                function if function_words(function).is_some() => {
                    function_words(function).unwrap_or_default().to_owned()
                }
                reference if reference.starts_with(|first: char| first.is_ascii_alphabetic()) => {
                    self.formula_reference(reference)
                }
                other => other.to_owned(),
            };
            if glue_next {
                match words.last_mut() {
                    Some(last) => last.push_str(&word),
                    None => words.push(word),
                }
                glue_next = false;
            } else {
                words.push(word);
            }
        }
        if words.is_empty() {
            BLANK_TEXT.to_owned()
        } else {
            words.join(" ")
        }
    }

    /// Returns the words of a reference in a formula: `my.x`, `my_new.x`, `l2`, `l2.amount`, or a parameter id.
    fn formula_reference(&self, reference: &str) -> String {
        let (head, tail) = reference.split_once('.').unwrap_or((reference, ""));
        match head {
            "my" => return format!("my {}", self.property_name(Some(tail))),
            "my_new" => return format!("my new {}", self.property_name(Some(tail))),
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

    /// Returns the name of state `id` of property `about`, or of the first property with such a state when `about`
    /// names none, as [`shared_state_name`] writes it.
    ///
    /// An unknown state reads as its id, and `None` as [`BLANK_TEXT`].
    fn state_name(&self, about: Option<&str>, id: Option<&str>) -> String {
        let Some(id) = id else {
            return BLANK_TEXT.to_owned();
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
            return id.to_owned();
        };
        let Some(state) = owner.states.iter().find(|state| state.id == id) else {
            return id.to_owned();
        };
        let shared = self.all_properties().any(|property| {
            property.id != owner.id
                && property.property_type == PropertyType::States
                && property.states.iter().any(|other| other.name == state.name)
        });
        shared_state_name(&owner.name, &state.name, shared)
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

/// Returns whether a line of `kind` works out a result, and so starts with its name.
fn is_result_kind(kind: LineKind) -> bool {
    matches!(
        kind,
        LineKind::Count
            | LineKind::LookAround
            | LineKind::PickCell
            | LineKind::LookAtCell
            | LineKind::Chance
            | LineKind::Rate
            | LineKind::RandomNumber
            | LineKind::RandomDirection
            | LineKind::Compare
            | LineKind::AllOf
            | LineKind::AnyOf
            | LineKind::NoneOf
            | LineKind::Choose
            | LineKind::DependingOn
            | LineKind::Calculate
    )
}

/// Returns whether `id` has the form of a line id, `l` and a number.
fn is_line_id(id: &str) -> bool {
    id.strip_prefix('l')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Returns the words of a literal: a number in the sheet's number form, a percent as typed ("30%"), "yes" or "no".
fn literal_words(literal: SheetLiteral) -> String {
    let number = |value: f64| format_number(value).unwrap_or_else(|| value.to_string());
    match literal {
        SheetLiteral::Number(value) => number(value),
        SheetLiteral::Percent(value) => format!("{}%", number(value)),
        SheetLiteral::Bool(true) => "yes".to_owned(),
        SheetLiteral::Bool(false) => "no".to_owned(),
    }
}

/// Returns the word of a Steer or Move line's mode: "by", "towards" or "to".
fn mode_words(mode: Option<LineMode>) -> &'static str {
    mode.map_or(BLANK_TEXT, LineMode::as_str)
}

/// Returns the word of a summary of a Look around line, such as "average".
fn summary_word(kind: SummaryKind) -> &'static str {
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
