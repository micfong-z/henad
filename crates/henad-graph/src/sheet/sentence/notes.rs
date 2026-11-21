//! Notes under a line, in the order a line shows them, and the words of "otherwise".

use henad_core::metadata::TimeUnit;

use super::card::{DrawKind, OTHERWISE, otherwise_description, rate_unit_word};
use super::grammar::{number_list, plural, upper_first, with_article, word_list};

// Notes under a line, in the order a line shows them

/// Returns the skip note of a line whose first empty blank wants `want`, such as "Not run yet: choose a state", where
/// `want` is [`Blank::want`] or [`LineSelect::words`].
///
/// [`Blank::want`]: super::Blank::want
/// [`LineSelect::words`]: super::LineSelect::words
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
    format!("The result {name} is only for lines inside its When. Choose another value.")
}

/// Note under an outside test with one side filled.
pub const ONE_SIDED_RANGE: &str = "Only one side is set. Use a plain comparison instead.";

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
/// `values` are the values of the count that no line applies to, `property` is the property those elements keep, and
/// `thing` names one of what the card runs on. For example "None of lines 2 and 3 applies when B in my block is 4 or
/// 5. Those cells keep their Opinion."
pub fn gap_note(lines: &[usize], count: &str, values: &[u32], property: &str, thing: &str) -> String {
    format!(
        "None of lines {} applies when {count} is {}. Those {} keep their {property}.",
        number_list(lines),
        count_values_words(values),
        plural(thing)
    )
}

/// Returns the coverage note under the last of the Become lines `lines` that together set every value of the count
/// `count`.
///
/// `ends` holds each end state with the values of the count that give it, and `thing` names one of what the card runs
/// on. For example "Lines 2 and 3 cover every value of B in my block. A cell ends the tick A when it is 0 to 4 and B
/// when it is 5 to 9."
pub fn coverage_note(lines: &[usize], count: &str, ends: &[(&str, &[u32])], thing: &str) -> String {
    let ends: Vec<String> = ends
        .iter()
        .map(|(state, values)| format!("{state} when it is {}", count_values_words(values)))
        .collect();
    format!(
        "Lines {} cover every value of {count}. {} ends the tick {}.",
        number_list(lines),
        upper_first(&with_article(thing)),
        word_list(&ends)
    )
}

/// Returns the order note under the last of the Become lines `lines`, which change elements in the old states `states`
/// on a card of which `thing` names one.
///
/// For example "Lines 2 and 3 change different cells (Dead and Alive), so their order does not matter."
pub fn order_note(lines: &[usize], states: &[&str], thing: &str) -> String {
    let states: Vec<String> = states.iter().map(|state| (*state).to_owned()).collect();
    format!(
        "Lines {} change different {} ({}), so their order does not matter.",
        number_list(lines),
        plural(thing),
        word_list(&states)
    )
}

/// Returns the note under a When whose lines all change from `state`, which the lines `lines` already say.
///
/// For example "Line 3 already says from Alive, so this When repeats it.", or "Lines 3 and 4 already say from Alive,
/// so this When repeats it."
pub fn when_repeats_note(lines: &[usize], state: &str) -> String {
    let who = if lines.len() == 1 {
        format!("Line {} already says", number_list(lines))
    } else {
        format!("Lines {} already say", number_list(lines))
    };
    format!("{who} from {state}, so this When repeats it.")
}

/// Label of the button that removes a When and keeps its lines.
pub const REMOVE_THIS_WHEN: &str = "Remove this When";

/// Returns the accessible name of the button that removes a When and keeps its lines, such as "Line 2: remove this
/// When and keep the lines inside", where `prefix` is the When's line prefix.
pub fn remove_when_name(prefix: &str) -> String {
    format!("{prefix}: remove this When and keep the lines inside")
}

/// Returns the note under a When around the one change on line `number`.
pub fn only_one_inside_note(number: usize) -> String {
    format!("Only line {number} is inside. Move this condition into that line's own when.")
}

/// Returns the label of the button that moves a When's condition into the change on line `number`.
pub fn move_into_label(number: usize) -> String {
    format!("Move it into line {number}")
}

/// Returns the accessible name of the button that moves a When's condition into line `inside`, such as "Line 4: move
/// it into line 5", where `prefix` is the When's line prefix.
pub fn move_into_name(prefix: &str, inside: usize) -> String {
    format!("{prefix}: move it into line {inside}")
}

/// Returns the note under a change or When inside When line `number`, whose condition reads `condition`.
///
/// For example "Only when line 4 holds: my Health is Recovered.", or "Only when line 4 holds." while `condition` is
/// `None`, a blank condition.
pub fn only_when_note(number: usize, condition: Option<&str>) -> String {
    match condition {
        Some(condition) => format!("Only when line {number} holds: {condition}."),
        None => format!("Only when line {number} holds."),
    }
}

/// Returns the note under a result line inside When line `number` on a card of which `thing` names one.
pub fn worked_out_everywhere_note(number: usize, thing: &str) -> String {
    format!(
        "Worked out for every {thing}, whether or not line {number} holds. Only lines inside line {number} can use it."
    )
}

/// Note under a When that holds no line.
pub const NOTHING_INSIDE: &str = "Nothing inside yet.";

// "otherwise"

/// Returns the words before a change that a higher line overrides, such as "otherwise (when line 2 does not apply)",
/// where `lines` holds the numbers of the higher lines.
pub fn otherwise_words(lines: &[usize]) -> String {
    format!("{OTHERWISE} ({})", otherwise_description(lines))
}

/// Places of the higher lines of an "otherwise" against the card's Repeat lines, which decide the words of
/// [`otherwise_tooltip`] on the line that wins.
///
/// Inside a Repeat the first time round that a change applies decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtherwiseRepeat {
    /// No higher line shares a Repeat with the line or with another higher line. The highest line that applies wins.
    Apart,
    /// A higher line sits in the line's own Repeat. The highest line wins among changes that apply at the same time.
    Own,
    /// Two or more higher lines share a Repeat that the line sits outside of. Between those lines, the first time
    /// round that one applies decides.
    Higher,
}

/// Returns the tooltip of the "otherwise" words, where `lines` holds the higher lines.
///
/// `property` is the property the lines change, `state` the state one `thing` must be in for both to apply, and
/// `thing` names one of what the card runs on. For example "Line 2 also changes Cover for a cell that is Tree. When
/// both apply, line 2 wins."
///
/// `repeat` places the higher lines against the card's Repeat lines. For [`OtherwiseRepeat::Own`] the tooltip says
/// the highest line wins at the same time, and for [`OtherwiseRepeat::Higher`] that the first time round decides
/// between lines in one Repeat.
pub fn otherwise_tooltip(
    lines: &[usize],
    property: &str,
    state: Option<&str>,
    thing: &str,
    repeat: OtherwiseRepeat,
) -> String {
    let who = if lines.len() == 1 {
        format!("Line {} also changes", number_list(lines))
    } else {
        format!("Lines {} also change", number_list(lines))
    };
    let for_whom = state
        .map(|state| format!(" for {} that is {state}", with_article(thing)))
        .unwrap_or_default();
    let when = if repeat == OtherwiseRepeat::Own {
        " at the same time"
    } else {
        ""
    };
    let wins = if lines.len() == 1 {
        format!("When both apply{when}, line {} wins.", number_list(lines))
    } else if repeat == OtherwiseRepeat::Higher {
        "When more than one applies, the line nearest the top of the card wins. Between lines in one Repeat, the \
         first time round that one applies decides."
            .to_owned()
    } else {
        format!("When more than one applies{when}, the line nearest the top of the card wins.")
    };
    format!("{who} {property}{for_whom}. {wins}")
}

/// Returns the notice shown the first time in a session that a line starts with "otherwise".
///
/// `prefix` is the line prefix, such as "Line 6" or "Clear line 2", and the tooltip of [`otherwise_tooltip`] follows
/// the first sentence.
pub fn otherwise_notice(
    prefix: &str,
    lines: &[usize],
    property: &str,
    state: Option<&str>,
    thing: &str,
    repeat: OtherwiseRepeat,
) -> String {
    format!(
        "{prefix} now starts with otherwise. {}",
        otherwise_tooltip(lines, property, state, thing, repeat)
    )
}
