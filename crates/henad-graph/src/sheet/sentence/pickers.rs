//! Words of the card's pickers, their messages, typed numbers and result names.

use henad_core::metadata::TimeUnit;

use super::card::{BLANK_TEXT, DrawKind, rate_unit_word};
use super::sentences::summary_word;
use crate::sheet::document::{SheetLiteral, SummaryKind};

// Pickers

/// Number box of a picker of a number.
pub const NUMBER_BOX_HINT: &str = "Type a number, then press Enter.";

/// Number box of a picker of a chance.
pub const CHANCE_BOX_HINT: &str = "Type a chance such as 0.3 or 30%, then press Enter.";

/// Number box of a picker of the share of a mark kept each tick.
pub const SHARE_BOX_HINT: &str = "Type a share such as 0.99 or 99%, then press Enter.";

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

/// Returns the heading of the group of the properties of the thing a card runs over, such as "This cell's properties"
/// or "This bird's properties", where `thing` is the word for one of the thing.
pub fn this_things_properties(thing: &str) -> String {
    format!("This {thing}'s properties")
}

/// Returns the grey words beside an item of the properties of the thing a card runs over, such as "this bird's
/// Velocity".
pub fn this_things_property(thing: &str, property: &str) -> String {
    format!("this {thing}'s {property}")
}

/// Returns an item of the value a property takes from the changes above a line, such as "my new Velocity".
pub fn my_new_property_item(property: &str) -> String {
    format!("my new {property}")
}

/// Returns the grey words beside "my new Velocity": "this bird's Velocity after line 6", where `number` is the number
/// of the last line that changes the property.
pub fn my_new_detail(thing: &str, property: &str, number: usize) -> String {
    format!("this {thing}'s {property} after line {number}")
}

/// Returns an item of the length of an arrow property, such as "length of my Velocity".
pub fn length_of_item(property: &str) -> String {
    format!("length of my {property}")
}

/// Returns the heading of the group of what a look at agents reads from each agent, such as "Each bird looked at".
pub fn each_looked_at(thing: &str) -> String {
    format!("Each {thing} looked at")
}

/// Returns an item of a property of each agent looked at, such as "their Velocity".
pub fn their_property_item(property: &str) -> String {
    format!("their {property}")
}

/// Words of a Look at a cell line that looks at the cell an agent is on.
pub const LOOK_AT_HERE: &str = "the cell I am on";

/// Words of a Look at a cell line that looks at the cell ahead of an agent.
pub const LOOK_AT_AHEAD: &str = "the cell ahead of me";

/// Words of a Look at a cell line that looks at a place, before the place.
pub const LOOK_AT_PLACE: &str = "the cell at";

/// Heading of the group of new values a picker of a value offers on a card over agents.
pub const OR_A_NEW_VALUE: &str = "Or a new value";

/// Heading of the group of the ground's properties in a picker of a value or of a ground property.
pub const GROUND_PROPERTIES: &str = "Ground properties";

/// Heading of the group of events in a picker of an event.
pub const EVENTS: &str = "Events";

/// Heading of the group of ways to sum the agents up in the picker of a new summary.
pub const SUMMARIES: &str = "Summaries";

/// Title of the picker of a new summary of a look at agents.
pub const CHOOSE_A_SUMMARY: &str = "Choose a summary";

/// Returns the accessible name of the "+" after the summaries of a look at agents, such as "Line 1: add another
/// summary".
pub fn add_summary_name(prefix: &str) -> String {
    format!("{prefix}: add another summary")
}

/// Returns the accessible name of the "+" after the rows of a Choose line, such as "Line 7: add another row".
pub fn add_choose_row_name(prefix: &str) -> String {
    format!("{prefix}: add another row")
}

/// Returns an item of the picker of a new summary, such as "average of ..." or "count".
pub fn summary_item(kind: SummaryKind) -> String {
    match kind {
        SummaryKind::Count => summary_word(kind).to_owned(),
        _ => format!("{} of {BLANK_TEXT}", summary_word(kind)),
    }
}

/// Item of a summary's picker that removes the summary.
pub const REMOVE_SUMMARY: &str = "Remove this summary";

/// Returns the tool of a look at agents' menu that counts the agent itself among those it looks at, such as "Include
/// me in line 1", where `word` is the line word.
pub fn include_me_tool(word: &str) -> String {
    format!("Include me in {word}")
}

/// Returns the tool of a look at agents' menu that leaves the agent itself out of those it looks at, such as "Exclude
/// me from line 1", where `word` is the line word.
pub fn leave_me_out_tool(word: &str) -> String {
    format!("Exclude me from {word}")
}

/// No ground property for a picker of a ground property.
pub const NO_GROUND_PROPERTY: &str = "No ground property yet. Add one under Ground on the sheet.";

/// No event for a picker of an event.
pub const NO_EVENT: &str = "No event yet. Add one under Events on the sheet.";

/// No place for a Move to line.
pub const NO_PLACE: &str = "No cell picked above this line yet. Make one with + Look around above this line.";

/// A typed number of times that is no whole number from 0.
pub const TYPE_TIMES: &str = "Type a non-negative integer, such as 4.";

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
            "The rate grows with the chosen count. With for each infected neighbour, a cell with 2 infected neighbours \
             gets twice the rate."
        }
        DrawKind::Chance => "Each one counted is an independent trial with this chance.",
    }
}

/// Item of the "for each" picker once "for each" is on.
pub const REMOVE_FOR_EACH: &str = "Remove for each";

// Messages of a picker with nothing to offer

/// No number result above the line.
pub const NO_NUMBER_RESULT: &str = "No number result above this line yet.";
/// No property for the value left of a comparison.
pub const NO_PROPERTY: &str = "No property yet.";
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
pub const CHANCE_OUT_OF_RANGE: &str = "The chance must be from 0 to 1, or from 0% to 100%.";
/// A typed rate below 0.
pub const RATE_BELOW_ZERO: &str = "The rate must be at least 0.";
/// Typed text that is not one number.
pub const TYPE_ONE_NUMBER: &str = "Type one number, such as 3 or 0.25.";
/// A list typed in a blank of one number.
pub const LIST_IN_ONE_NUMBER: &str = "For a list such as 2, 3, choose is one of the numbers or is none of the numbers \
                                      first.";

/// Returns the number typed as `text`, or `None` for text that is not one number.
///
/// A trailing "%" makes the number a percent, the number divided by 100. Spaces around the number and before the "%"
/// are ignored.
///
/// ```
/// use henad_graph::sheet::document::SheetLiteral;
/// use henad_graph::sheet::sentence::read_typed_number;
///
/// assert_eq!(read_typed_number(" 0.3 "), Some(SheetLiteral::Number(0.3)));
/// assert_eq!(read_typed_number("30%"), Some(SheetLiteral::Percent(30.0)));
/// assert_eq!(read_typed_number("2, 3"), None);
/// ```
pub fn read_typed_number(text: &str) -> Option<SheetLiteral> {
    let text = text.trim();
    let (number, percent) = match text.strip_suffix('%') {
        Some(number) => (number.trim_end(), true),
        None => (text, false),
    };
    // Rust reads "inf" and "nan" as numbers, and a sheet holds finite numbers only.
    let starts_well = number
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.'));
    let value: f64 = number
        .parse()
        .ok()
        .filter(|value: &f64| value.is_finite() && starts_well)?;
    Some(if percent {
        SheetLiteral::Percent(value)
    } else {
        SheetLiteral::Number(value)
    })
}

/// Returns the reason an edit is refused that would keep `value`, a number that is not finite, on the sheet.
///
/// It reads "that number is too large" for an infinity and "that number cannot be worked out" for NaN. The
/// reason is a clause, which the editor starts with a capital and ends with a full stop.
pub fn number_not_kept(value: f64) -> &'static str {
    if value.is_nan() {
        "that number cannot be worked out"
    } else {
        "that number is too large"
    }
}

/// Returns the message on a side of the world typed as anything but a whole number from 1 to `max` cells, such as
/// "Type a size from 1 to 16384 cells.".
pub fn world_side_out_of_range(max: u32) -> String {
    format!("Type a size from 1 to {max} cells.")
}

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

/// Returns the error of a field outside the cards left empty, such as "Health has an empty field. Type a chance in
/// it."
///
/// `name` is the name of the property, measure or button that holds the field, or the heading of its section, and
/// `want` is [`Blank::want`](super::Blank::want).
pub fn empty_field(name: &str, want: &str) -> String {
    format!("{name} has an empty field. Type {want} in it.")
}
