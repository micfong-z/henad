//! Errors and refusals of a sheet, the words of a look at agents that costs too much, and fire counts.

use super::grammar::{number_list, plural, upper_first, word_list};

// Errors on a rate in a button's card and on a second agent kind

/// Error on a rate in the card of a button.
pub const RATE_IN_BUTTON_CARD: &str = "A button's card has no duration, so it takes chances, not rates.";

/// Error on a second kind of agent.
pub const ONE_AGENT_KIND: &str = "One kind of agent per model in this version.";

// Errors on the lines and worlds a sheet can hold that this version cannot run yet

/// Error on a Repeat line.
pub const NOT_YET_REPEAT: &str = "Repeat is not supported yet.";

/// Error on a Keep between, When, Repeat, Leave, Record or Move to line inside a Repeat.
pub const NOT_YET_REPEAT_CHANGE: &str =
    "Keep between, When, Repeat, Leave, Record and Move to inside a Repeat are not supported yet.";

/// Error on a world of agents with walls at its edges whose agents move by an arrow.
pub const NOT_YET_AGENT_WALLS: &str = "Walls around agents that move by an arrow are not supported yet.";

/// Error on a Move by line in a sheet whose ground blocks some states.
pub const NOT_YET_BLOCKED_MOVE_BY: &str = "Blocked ground under agents that move by an arrow is not supported yet.";

/// Error on a line too long for Henad to run, such as one that tests against hundreds of values.
pub const LINE_TOO_LONG: &str = "This line is too long. Use shorter lists in it.";

/// Error on a line whose number is past the largest a line can have.
pub const LINE_NUMBER_TOO_LARGE: &str = "This line has too large a number.";

/// Error on a sheet too large for Henad to run outside its lines.
pub const SHEET_TOO_LARGE: &str = "The sheet is too large. Use fewer properties, measures or buttons.";

// Errors on the ground and the places agents move to

/// Error on a start on the ground in a state that the ground does not paint on single cells.
pub const ON_GROUND_CELLS: &str = "Agents can start only on a state the ground paints on single cells.";

/// Error on a look at the cell ahead in a sheet whose agents do not move to cells.
pub const AHEAD_NEEDS_CELLS: &str = "The cell ahead needs agents that move to cells.";

/// Error on a Count, Look around or Pick a cell line inside a Repeat.
pub const LOOK_IN_REPEAT: &str = "A look around, a count or a pick of a cell cannot sit inside a Repeat.";

/// Error on a Set inside a Repeat whose value reads a line inside that Repeat.
pub const SET_IN_REPEAT: &str =
    "A Set inside a Repeat sets one value every time, so its value cannot read a line inside the Repeat.";

/// Error on a comparison of an arrow with a number.
pub const ARROW_COMPARED_WITH_NUMBER: &str = "An arrow cannot be compared with a number. Compare its length instead.";

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

pub use crate::wgsl::fault_names::{look_met_too_many, look_searches_too_far};

/// Returns the error on looks at agents named `look` that cost too much at `count` agents.
///
/// `things` is the plural of the kind of agent. For example "Line 1, Flockmates: looks at too many birds at 1,000,000
/// birds. Use a shorter distance or fewer birds."
pub fn look_costs_too_much(look: &str, things: &str, count: u64) -> String {
    format!(
        "{}: looks at too many {things} at {} {things}. Use a shorter distance or fewer {things}.",
        upper_first(look),
        whole_number_text(count)
    )
}

/// Returns the error on the card headed `heading` that costs too much at `count` agents.
///
/// `things` is the plural of the kind of agent. For example "Each tick, every bird: too much work at 1,000,000 birds.
/// Use fewer birds."
pub fn card_costs_too_much(heading: &str, things: &str, count: u64) -> String {
    format!(
        "{heading}: too much work at {} {things}. Use fewer {things}.",
        whole_number_text(count)
    )
}

/// Returns the error on the line named `line` whose work for one `thing` in one run of its card passes the limit,
/// with `fix` the change that fits.
///
/// `line` is a name as [`look_name`] writes it. For example "Line 1: too much work for one walker. Set a smaller
/// maximum for Steps per tick."
pub fn line_costs_too_much(line: &str, thing: &str, fix: &str) -> String {
    format!("{}: too much work for one {thing}. {fix}", upper_first(line))
}

/// Returns the change to the Repeat over the parameter named `parameter` that fits: "Set a smaller maximum for Steps
/// per tick."
pub fn smaller_maximum(parameter: &str) -> String {
    format!("Set a smaller maximum for {parameter}.")
}

/// Change to a Repeat of a fixed number of times that fits.
pub const FEWER_TIMES: &str = "Repeat fewer times.";

/// Change to a card or a line other than a Repeat that fits.
pub const FEWER_LINES: &str = "Use fewer lines on this card.";

/// Returns the error on the starts of `count` agents that cost too much, `things` the plural of the kind of agent:
/// "Starting 1,000,000 birds takes too much work. Use fewer birds."
pub fn start_costs_too_much(things: &str, count: u64) -> String {
    format!(
        "Starting {} {things} takes too much work. Use fewer {things}.",
        whole_number_text(count)
    )
}

/// Returns the fire count `count` of a line as the card shows it beside the line, such as "30,877".
pub fn fire_count(count: u64) -> String {
    whole_number_text(count)
}

/// Returns the description of the fire count `count` of the line named `prefix`, whose card runs on things called
/// `thing`, counted in tick `tick`, such as "Line 2 applied to 2 cells in tick 4.".
pub fn fire_count_description(prefix: &str, count: u64, thing: &str, tick: u64) -> String {
    let things = if count == 1 { thing.to_owned() } else { plural(thing) };
    format!(
        "{prefix} applied to {} {things} in tick {tick}.",
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
