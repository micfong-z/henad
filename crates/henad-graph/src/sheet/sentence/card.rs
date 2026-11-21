//! Fixed words and templates of the card: its frame, the add menus, the words of line sentences, comparisons,
//! conditions and blanks, the Lines help and the words sheet mode never shows.

use henad_core::metadata::TimeUnit;

use super::grammar::{number_list, plural};
use crate::sheet::document::{Around, ChipRelation, CompareTest, LineKind};

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
pub const LAW: &str = "Everyone reads the world as it was at the start of the tick. All changes are applied together \
                       at the end of the tick. A property that no line changes is left unchanged.";

/// Label of the add button of agent cards that adds a Repeat line.
pub const REPEAT_BUTTON: &str = "+ Repeat...";

/// Accessible name of the "?" beside the add buttons, which opens the Lines help.
pub const HELP_LINES_NAME: &str = "Help: lines";

/// Note above the add buttons once the card holds a When line.
pub const ADD_AT_END_NOTE: &str = "New lines will be added at the end of the card.";

/// Returns the label of the row that ends When line `number`, such as "+ Add inside line 4".
pub fn add_inside_label(number: usize) -> String {
    format!("+ Add inside line {number}")
}

/// Returns the title of the menu of the row that ends When line `number`, such as "Add a line inside line 4". The row
/// itself is named by its label, [`add_inside_label`].
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

/// Returns the accessible name of a line on the card, such as "Line 2: Become Alive from Dead, when Alive neighbours is
/// 3", where `prefix` is the line prefix and `sentence` the line's sentence.
pub fn line_name(prefix: &str, sentence: &str) -> String {
    format!("{prefix}: {sentence}")
}

/// Returns the name of the menu of a line's tools, such as "Line 2 actions", where `prefix` is the line prefix.
pub fn line_actions_name(prefix: &str) -> String {
    format!("{prefix} actions")
}

/// Returns the tool that renames a result line, such as "Rename line 2", where `word` is the line word.
pub fn rename_line(word: &str) -> String {
    format!("Rename {word}")
}

/// Returns the name of a result line's name on its card, a button that renames the line, such as "Line 1: rename Alive
/// neighbours", where `prefix` is the line prefix and `name` the result's name.
pub fn rename_result_name(prefix: &str, name: &str) -> String {
    format!("{prefix}: rename {name}")
}

/// Returns the tool that moves a line up, such as "Move line 2 up", where `word` is the line word.
pub fn move_line_up(word: &str) -> String {
    format!("Move {word} up")
}

/// Returns the tool that moves a line down, such as "Move line 2 down", where `word` is the line word.
pub fn move_line_down(word: &str) -> String {
    format!("Move {word} down")
}

/// Tooltip of the disabled Move up on the first line of a card.
pub const MOVE_UP_FIRST: &str = "The first line of a card cannot move up.";

/// Tooltip of the disabled Move down on the last line of a card.
pub const MOVE_DOWN_LAST: &str = "The last line of a card cannot move down.";

/// Returns the tool that deletes a line, such as "Delete line 2", where `word` is the line word.
pub fn delete_line(word: &str) -> String {
    format!("Delete {word}")
}

/// Tooltip of the tool that deletes a When line.
pub const DELETE_WHEN_TOOLTIP: &str = "Delete. Lines inside will be kept.";

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
    /// "+ Repeat...", on a card over agents alone.
    Repeat,
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

    /// Every add button of a card over agents, in the order the card shows them.
    pub const AGENTS: [Self; 7] = [
        Self::LookAround,
        Self::Chance,
        Self::YesOrNo,
        Self::Calculate,
        Self::Change,
        Self::When,
        Self::Repeat,
    ];

    /// Groups of the menu of a block's add row on a card over agents, in order.
    pub const AGENTS_INSIDE_ORDER: [Self; 7] = [
        Self::Change,
        Self::When,
        Self::Repeat,
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
            Self::Repeat => REPEAT_BUTTON,
        }
    }

    /// Tooltip of the button on a card over cells.
    pub fn tooltip(self) -> &'static str {
        match self {
            Self::LookAround => "Count or total of the cells around me (neighbours)",
            Self::Chance => "Change a state by chance or at a rate",
            Self::YesOrNo => "Compare values, or join yes or no results",
            Self::Calculate => "Work out a number from two numbers",
            Self::Change => "Change a state or a number",
            Self::When => "Give several lines one shared condition",
            Self::Repeat => "Run some lines several times each tick",
        }
    }

    /// Returns the tooltip of the button on a card over agents when `agents` holds, and over cells otherwise.
    pub fn tooltip_for(self, agents: bool) -> &'static str {
        match (agents, self) {
            (true, Self::LookAround) => "Count or sum up the agents near me, or look at a cell",
            (true, Self::Chance) => "Change a state by chance, or draw a random number or direction",
            (true, Self::Calculate) => "Work out a number, or choose between values",
            (true, Self::Change) => "Change a state, a number, a direction or a place, or leave a mark",
            _ => self.tooltip(),
        }
    }

    /// Returns the button whose menus add a line of `kind`, the group the kind belongs to.
    ///
    /// Note that a Become belongs to Change wherever it was added, "Become, with chance" from "+ Chance" included, and
    /// the condition "always", which no line holds alone, to Yes or no.
    pub fn for_kind(kind: LineKind) -> Self {
        match kind {
            LineKind::Count | LineKind::LookAround | LineKind::PickCell | LineKind::LookAtCell => Self::LookAround,
            LineKind::Chance | LineKind::Rate | LineKind::RandomNumber | LineKind::RandomDirection => Self::Chance,
            LineKind::Compare | LineKind::AllOf | LineKind::AnyOf | LineKind::NoneOf | LineKind::Always => {
                Self::YesOrNo
            }
            LineKind::Calculate | LineKind::Choose | LineKind::DependingOn => Self::Calculate,
            LineKind::Become
            | LineKind::Set
            | LineKind::Steer
            | LineKind::KeepBetween
            | LineKind::Move
            | LineKind::Leave
            | LineKind::Record => Self::Change,
            LineKind::When => Self::When,
            LineKind::Repeat => Self::Repeat,
        }
    }

    /// Heading of the button's group in the menu of a When block's add row: "Change", "When", "Chance", "Look
    /// around", "Yes or no" or "Calculate".
    pub fn group(self) -> &'static str {
        match self {
            Self::LookAround => "Look around",
            Self::Chance => "Chance",
            Self::YesOrNo => "Yes or no",
            Self::Calculate => "Calculate",
            Self::Change => "Change",
            Self::When => "When",
            Self::Repeat => "Repeat",
        }
    }

    /// Heading of the button's menu on a card over cells.
    pub fn heading(self) -> &'static str {
        match self {
            Self::LookAround => "Look at the cells around me (neighbours)",
            Self::Chance => "Change a state by chance",
            Self::YesOrNo => "Make a yes or no result",
            Self::Calculate => "Work out a number",
            Self::Change => "Change one of my properties",
            Self::When => "Several lines that share one condition",
            Self::Repeat => "Lines that run several times each tick",
        }
    }

    /// Returns the heading of the button's menu on a card over agents when `agents` holds, and over cells otherwise.
    pub fn heading_for(self, agents: bool) -> &'static str {
        match (agents, self) {
            (true, Self::LookAround) => "Look at the agents near me, or at a cell",
            (true, Self::Chance) => "Change a state by chance, or draw a random value",
            (true, Self::Calculate) => "Work out a value",
            (true, Self::Change) => "Change one of my properties, or leave a mark",
            _ => self.heading(),
        }
    }

    /// Items of the button's menu on a card over cells, in order.
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
            Self::Repeat => &[AddItem::Repeat],
        }
    }

    /// Returns the items of the button's menu on a card over agents when `agents` holds, and over cells otherwise, in
    /// order.
    ///
    /// A card over agents takes chances and no rates.
    pub fn items_for(self, agents: bool) -> &'static [AddItem] {
        match (agents, self) {
            (true, Self::LookAround) => &[
                AddItem::CountAgents,
                AddItem::LookAroundAgents,
                AddItem::PickCell,
                AddItem::CellHereOrAhead,
            ],
            (true, Self::Chance) => &[
                AddItem::BecomeWithChance,
                AddItem::WithChance,
                AddItem::RandomNumber,
                AddItem::RandomDirection,
            ],
            (true, Self::Calculate) => &[AddItem::Calculate, AddItem::Choose, AddItem::DependingOn],
            (true, Self::Change) => &[
                AddItem::Become,
                AddItem::Set,
                AddItem::Steer,
                AddItem::KeepBetween,
                AddItem::MoveBy,
                AddItem::MoveTo,
                AddItem::Leave,
                AddItem::Record,
            ],
            _ => self.items(),
        }
    }
}

/// Heading of the group of the Chance menu of a card over agents that adds a random number or direction, above
/// [`AddItem::RandomNumber`].
pub const RANDOM_VALUE_GROUP: &str = "Or a random value";

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
    /// Count of the agents within a distance, under "+ Look around" on a card over agents.
    CountAgents,
    /// Look around at the agents within a distance, under "+ Look around" on a card over agents.
    LookAroundAgents,
    /// Pick a neighbouring cell of the ground, under "+ Look around" on a card over agents.
    PickCell,
    /// The cell here or ahead, under "+ Look around" on a card over agents.
    CellHereOrAhead,
    /// A random number, under "+ Chance" on a card over agents.
    RandomNumber,
    /// A random direction, under "+ Chance" on a card over agents.
    RandomDirection,
    /// Choose, under "+ Calculate" on a card over agents.
    Choose,
    /// Depending on my state, under "+ Calculate" on a card over agents.
    DependingOn,
    /// Steer, under "+ Change" on a card over agents.
    Steer,
    /// Keep between, under "+ Change" on a card over agents.
    KeepBetween,
    /// Move by an arrow, under "+ Change" on a card over agents.
    MoveBy,
    /// Move to a place, under "+ Change" on a card over agents.
    MoveTo,
    /// Leave a mark on the ground, under "+ Change" on a card over agents.
    Leave,
    /// Record an event, under "+ Change" on a card over agents.
    Record,
    /// Repeat, under "+ Repeat..." on a card over agents.
    Repeat,
}

impl AddItem {
    /// Label of the item, which is also its accessible name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Count | Self::CountAgents => "Count",
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
            Self::LookAroundAgents => "Look around",
            Self::PickCell => "Pick a cell",
            Self::CellHereOrAhead => "The cell here or ahead",
            Self::RandomNumber => "Random number",
            Self::RandomDirection => "Random direction",
            Self::Choose => "Choose",
            Self::DependingOn => "Depending on",
            Self::Steer => "Steer",
            Self::KeepBetween => "Keep between",
            Self::MoveBy => "Move by",
            Self::MoveTo => "Move to",
            Self::Leave => "Leave",
            Self::Record => "Record",
            Self::Repeat => "Repeat",
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
            Self::CountAgents => "count of the agents within ... whose ... is ...".to_owned(),
            Self::LookAroundAgents => {
                "look at the agents within ...: average of ..., total of ... and others".to_owned()
            }
            Self::PickCell => "the open neighbouring cell with the most ... (ties at random)".to_owned(),
            Self::CellHereOrAhead => "the cell I am on, or the cell ahead of me".to_owned(),
            Self::RandomNumber => "a random number between ... and ...".to_owned(),
            Self::RandomDirection => "a random direction of length ...".to_owned(),
            Self::Choose => "choose, first that applies: ..., when ...; otherwise ...".to_owned(),
            Self::DependingOn => "depending on my ...: a value for each state".to_owned(),
            Self::Steer => "Steer ... by or towards ..., strength ...".to_owned(),
            Self::KeepBetween => "Keep ... between ... and ...".to_owned(),
            Self::MoveBy => "Move by ..., such as a random direction".to_owned(),
            Self::MoveTo => "Move to a cell a line above picks".to_owned(),
            Self::Leave => "Leave ... on a ground property, when ...".to_owned(),
            Self::Record => "Record an event, when ...".to_owned(),
            Self::Repeat => "Repeat ... times: then add the lines to repeat".to_owned(),
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
///
/// [`Sentences`]: super::Sentences
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

/// Automatic name of the average offset to the agents looked at, the way to their centre.
pub const CENTER_SUMMARY: &str = "centre";

/// Automatic name of the total offset away from the agents looked at, a push away from them.
pub const PUSH_SUMMARY: &str = "push";

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
/// [`CompareTest::Outside`] reads "is less than ... or more than ...".
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

// Conditions and "Choose when"

/// The condition that always holds, a grey button after the comma of a change.
pub const ALWAYS: &str = "always";

/// Returns the accessible name of the "always" button, such as "Line 2: choose when, now always".
pub fn always_name(prefix: &str) -> String {
    format!("{prefix}: choose when, now always")
}

/// Tooltip of the "always" button.
pub const ALWAYS_TOOLTIP: &str = "No condition. Press to add one.";

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
    format!("Line {number} already has a condition. Choose always to use only that condition.")
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
            Self::Is => format!("{when} ... {} ...", comparison_words(CompareTest::Is, false)),
            Self::AtLeast => format!("{when} ... {} ...", comparison_words(CompareTest::AtLeast, false)),
            Self::AtMost => format!("{when} ... {} ...", comparison_words(CompareTest::AtMost, false)),
            Self::Outside => format!("{when} ... {}", comparison_words(CompareTest::Outside, false)),
            Self::WithChance => format!("{} ...", condition_head(ConditionKind::Chance, on_when_line)),
            Self::AtRate => format!("{} ... per ...", condition_head(ConditionKind::Rate, on_when_line)),
        }
    }

    /// Grey line under the item, `None` for an item without one.
    pub fn description(self) -> Option<&'static str> {
        match self {
            Self::Is => Some("The same as a value. Choose is not, is more than or another comparison on the line."),
            Self::AtLeast => Some("The value or more, such as 1 or more"),
            Self::AtMost => Some("The value or less, such as 4 or less"),
            Self::Outside => Some("Outside a range, such as less than 2 or more than 3"),
            Self::Always | Self::WithChance | Self::AtRate => None,
        }
    }
}

// Blanks and their accessible names

/// Kind of value a blank wants. It names the blank, titles its picker and words its skip note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// The number of times a Repeat runs its lines, a whole number.
    Times,
    /// A property of the ground.
    GroundProperty,
    /// An event.
    Event,
}

impl Blank {
    /// Every blank.
    pub const ALL: [Self; 19] = [
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
        Self::Times,
        Self::GroundProperty,
        Self::Event,
    ];

    /// Visible word of the blank, drawn in its dashed button.
    pub fn word(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::OldState => "old state",
            Self::Property => "property",
            Self::NumberProperty => "number property",
            Self::Value => "value",
            Self::Number | Self::LowerNumber | Self::UpperNumber | Self::Times => "number",
            Self::NumberList => "list of numbers",
            Self::StateList => "list of states",
            Self::Chance => "chance",
            Self::Rate => "rate",
            Self::Count => "count",
            Self::YesOrNo => "result",
            Self::YesOrNoList => "results",
            Self::When => "when",
            Self::GroundProperty => "ground property",
            Self::Event => "event",
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
            Self::Times => "how many times",
            Self::GroundProperty => "a ground property",
            Self::Event => "an event",
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
    /// Whether a Steer line steers by a value or towards it.
    SteerMode,
    /// Whether a Move line moves by an arrow or to a place.
    MoveMode,
    /// Rule a Pick a cell line picks by: the most, the least or a cell at random.
    Pick,
    /// The cell a Look at a cell line looks at.
    WhichCell,
    /// Kind of the summary at this index of a look at agents, the index counted from 0.
    LookSummary(usize),
}

impl LineSelect {
    /// Every select of fixed words. [`Self::LookSummary`] takes the summary's number in its name.
    pub const ALL: [Self; 9] = [
        Self::UnitOfTime,
        Self::Comparison,
        Self::WhichCells,
        Self::TotalOrAverage,
        Self::Operation,
        Self::SteerMode,
        Self::MoveMode,
        Self::Pick,
        Self::WhichCell,
    ];

    /// Words of the select, after the line prefix in its accessible name. A summary's select reads "summary" before
    /// its number.
    pub fn words(self) -> &'static str {
        match self {
            Self::UnitOfTime => "unit of time",
            Self::Comparison => "comparison",
            Self::WhichCells => "which cells",
            Self::TotalOrAverage => "total or average",
            Self::Operation => "operation",
            Self::SteerMode => "by or towards",
            Self::MoveMode => "by or to",
            Self::Pick => "most, least or random",
            Self::WhichCell => "which cell",
            Self::LookSummary(_) => "summary",
        }
    }

    /// Returns the accessible name of the select, such as "Line 2: unit of time", or "Line 1: summary 2" for the
    /// second summary of a look at agents.
    pub fn name(self, prefix: &str) -> String {
        match self {
            Self::LookSummary(index) => format!("{prefix}: {} {}", self.words(), index + 1),
            _ => format!("{prefix}: {}", self.words()),
        }
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

/// Words the optional "for each ..." blank shows.
pub const FOR_EACH_BLANK_WORDS: &str = "for each ...";

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
/// is the count's words after "for each" from [`for_each_words`](super::for_each_words).
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

/// Word "from" of a Become line, between its new state and its old state.
pub const FROM_WORD: &str = "from";

/// Returns the tooltip of the word "from" of a Become line on a card of which `thing` names one, such as "Only cells in
/// this state now change".
pub fn from_tooltip(thing: &str) -> String {
    format!("Only {} in this state now change", plural(thing))
}

/// Returns a state's name as chips and pickers show it: `state`, or "Status Alive" with `property` when another
/// property has a state of the same name.
pub fn shared_state_name(property: &str, state: &str, shared: bool) -> String {
    if shared {
        format!("{property} {state}")
    } else {
        state.to_owned()
    }
}

// The Lines help

/// Heading of the Lines help.
pub const LINES_HELP_HEADING: &str = "Lines";

/// Text of the Lines help.
pub const LINES_HELP: &str = "Every cell follows these lines each tick, from the top. A line either works out a \
                              result, which lines below it can use, or changes one of my properties. Everyone reads \
                              the world as it was at the start of the tick. All changes are applied together at the \
                              end of the tick. A property that no line changes is left unchanged. When two changes to \
                              one property both apply, the line nearest the top of the card wins, and the lower line \
                              shows \"otherwise\".";

/// Example of the Lines help.
pub const LINES_HELP_EXAMPLE: &str = "Alive neighbours = count of the 8 cells around me whose Status is Alive";

/// Text of the Lines help beside the card of a kind of agent.
pub const LINES_HELP_AGENTS: &str = "Every agent of this kind follows these lines each tick, from the top. A line \
                                     either works out a result, which lines below it can use, or changes one of my \
                                     properties. Everyone reads the world as it was at the start of the tick. All \
                                     changes are applied together at the end of the tick. A property that no line \
                                     changes is left unchanged. When two changes to one property both apply, the \
                                     line nearest the top of the card wins, and the lower line shows \"otherwise\". \
                                     Inside a Repeat, the first time a change applies decides, so a lower line that \
                                     applies earlier wins.";

/// Example of the Lines help beside the card of a kind of agent.
pub const LINES_HELP_AGENTS_EXAMPLE: &str =
    "Too close = look at birds within Protected range: total of offset away from them (push)";

/// Label before the example of a help popover.
pub const HELP_EXAMPLE_LABEL: &str = "Example:";

/// Button that closes a help popover.
pub const CLOSE_HELP: &str = "Close help";

/// Returns the accessible name of the help popover under `heading`, such as "World help" or "Lines help".
pub fn help_dialog_name(heading: &str) -> String {
    format!("{heading} help")
}

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

/// Returns whether `word` is one of [`FORBIDDEN_WORDS`] in any case, or one of them with an ending of a plural or a
/// verb: "s", "es", "d", "ed" or "ing", and "ies" for a final "y".
///
/// ```
/// use henad_graph::sheet::sentence::is_forbidden_word;
///
/// assert!(is_forbidden_word("Nodes"));
/// assert!(is_forbidden_word("compiled"));
/// assert!(is_forbidden_word("categories"));
/// assert!(!is_forbidden_word("portion"));
/// assert!(!is_forbidden_word("iron"));
/// ```
pub fn is_forbidden_word(word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    FORBIDDEN_WORDS.iter().any(|forbidden| {
        let forbidden = forbidden.to_ascii_lowercase();
        let Some(ending) = word.strip_prefix(forbidden.as_str()) else {
            return forbidden
                .strip_suffix('y')
                .is_some_and(|stem| word.strip_prefix(stem) == Some("ies"));
        };
        matches!(ending, "" | "s" | "es" | "d" | "ed" | "ing")
    })
}
