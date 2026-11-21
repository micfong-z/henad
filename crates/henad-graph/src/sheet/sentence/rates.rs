//! Words and notes of rates and chances, competing rates, the long tick and rate conversion.

use henad_core::metadata::TimeUnit;

use super::card::rate_unit_word;
use super::grammar::{drop_ordinary_capital, indefinite_article, number_list, upper_first, with_article};

// Rates and chances

/// Returns the start of a rate note: "Each tick", or "Each tick (1 day)" when one tick has the length `length`.
pub fn each_tick(length: Option<&str>) -> String {
    match length {
        Some(length) => format!("Each tick ({length})"),
        None => "Each tick".to_owned(),
    }
}

/// Returns the subject of a rate note on a Become line from `old_state` to `new_state`, on a card of which `thing`
/// names one.
///
/// It reads "a Susceptible cell becomes Infected" ("an" before a vowel), "a cell becomes Infected" from any state or
/// with the old state empty, or "a cell changes" with the new state empty.
pub fn become_subject(thing: &str, old_state: Option<&str>, new_state: Option<&str>) -> String {
    match (old_state, new_state) {
        (_, None) => format!("{} changes", with_article(thing)),
        (None, Some(new)) => format!("{} becomes {new}", with_article(thing)),
        (Some(old), Some(new)) => format!("{} {old} {thing} becomes {new}", indefinite_article(old)),
    }
}

/// Returns the subject of a rate note on a Set line, such as "a cell takes the new value".
pub fn set_subject(thing: &str) -> String {
    format!("{} takes the new value", with_article(thing))
}

/// Returns the subject of a rate note on When line `number`, such as "the lines inside line 4 apply to a cell".
pub fn when_subject(number: usize, thing: &str) -> String {
    format!("the lines inside line {number} apply to {}", with_article(thing))
}

/// Returns the subject of a rate note on the result `name`, such as "the result Recovery happens is yes for a cell".
pub fn result_subject(name: &str, thing: &str) -> String {
    format!("the result {name} is yes for {}", with_article(thing))
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

/// Opening words of the sentence of a rate note that gives the formula turning the rate into a chance.
pub const RATE_CONVERSION: &str = "The rate is converted to a chance by";

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
/// The rate is converted to a chance by 1 − e^(−0.1 × 1 day)."
///
/// `tick` is the length of one tick, `subject` the note's subject from [`become_subject`] and the like, and `chance`
/// and `rate` are formatted numbers.
pub fn rate_note(tick: &str, subject: &str, chance: &str, rate: &str) -> String {
    format!(
        "{}, {subject} with chance {chance}. {RATE_CONVERSION} 1 − e^(−{rate} × {tick}).",
        each_tick(Some(tick))
    )
}

/// Returns the note of a rate line counted for each `count`, with the chances `first` and `second` at 1 and at 2.
///
/// For example "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259 with 1 infected neighbour,
/// and 0.451 with 2. The rate is converted to a chance by 1 − e^(−0.3 × n × 1 day), where n is Infected neighbours."
pub fn rate_for_each_note(tick: &str, subject: &str, count: &str, first: &str, second: &str, rate: &str) -> String {
    format!(
        "{}, {subject} {}. {RATE_CONVERSION} 1 − e^(−{rate} × n × {tick}), where n is {count}.",
        each_tick(Some(tick)),
        count_chances(count, first, second)
    )
}

/// Returns the note of a rate line whose rate is a Calculate result of a fixed number times the count `count`.
///
/// `rate_words` reads the result, as "0.3 times Infected neighbours". For example "Each tick (1 day), a Susceptible
/// cell becomes Infected with chance 0.259 with 1 infected neighbour, and 0.451 with 2. The rate is converted to a
/// chance by 1 − e^(−r × 1 day), where r is 0.3 times Infected neighbours."
pub fn worked_out_rate_note(
    tick: &str,
    subject: &str,
    count: &str,
    first: &str,
    second: &str,
    rate_words: &str,
) -> String {
    format!(
        "{}, {subject} {}. {RATE_CONVERSION} 1 − e^(−r × {tick}), where r is {rate_words}.",
        each_tick(Some(tick)),
        count_chances(count, first, second)
    )
}

/// Returns the note of a rate line whose rate is any other worked-out result, read as `rate_words`, and counted for
/// each `count` when there is one.
///
/// For example "Each tick (1 day), a Susceptible cell becomes Infected with a chance that grows with r, where r is
/// Infected neighbours plus 0.3. The rate is converted to a chance by 1 − e^(−r × 1 day)."
pub fn growing_rate_note(tick: &str, subject: &str, rate_words: &str, count: Option<&str>) -> String {
    let head = each_tick(Some(tick));
    match count {
        None => format!(
            "{head}, {subject} with a chance that grows with r, where r is {rate_words}. {RATE_CONVERSION} 1 − \
             e^(−r × {tick})."
        ),
        Some(count) => format!(
            "{head}, {subject} with a chance that grows with r and n, where r is {rate_words} and n is {count}. \
             {RATE_CONVERSION} 1 − e^(−r × n × {tick})."
        ),
    }
}

/// Returns the note of a chance line counted for each `count`, with the chances `first` and `second` at 1 and at 2.
///
/// `tick` is the length of one tick, if it has one. For example "Each tick, a cell becomes B with chance 0.2 with 1 B
/// neighbour, and 0.36 with 2. Each one counted is an independent trial: 1 − (1 − 0.2)^n, where n is B neighbours." A
/// chance without "for each" has no note.
pub fn chance_for_each_note(tick: Option<&str>, subject: &str, count: &str, first: &str, second: &str) -> String {
    format!(
        "{}, {subject} {}. Each one counted is an independent trial: 1 − (1 − {first})^n, where n is {count}.",
        each_tick(tick),
        count_chances(count, first, second)
    )
}

/// Returns the note of a chance line counted for each `count` whose chance is a result, read as `chance_words`.
///
/// For example "Each tick, a cell becomes B with a chance that grows with n, where p is Spark chance and n is B
/// neighbours. Each one counted is an independent trial: 1 − (1 − p)^n."
pub fn growing_chance_note(tick: Option<&str>, subject: &str, chance_words: &str, count: &str) -> String {
    format!(
        "{}, {subject} with a chance that grows with n, where p is {chance_words} and n is {count}. Each one counted \
         is an independent trial: 1 − (1 − p)^n.",
        each_tick(tick)
    )
}

/// Returns the note of a group of competing rate lines that take elements out of `state`, on a card of which `thing`
/// names one.
///
/// `chance` is the chance of leaving, `shares` holds each new state with its share, and `rates` each line's rate, all
/// formatted. For example "Each tick (1 day), an Infected cell leaves Infected with chance 0.0952: Recovered 0.0476,
/// Dead 0.0476. The rate is converted to a chance by 1 − e^(−(0.05 + 0.05) × 1 day)."
pub fn competing_note(
    tick: &str,
    state: &str,
    chance: &str,
    shares: &[(&str, &str)],
    rates: &[&str],
    thing: &str,
) -> String {
    let shares: Vec<String> = shares
        .iter()
        .map(|(new_state, share)| format!("{new_state} {share}"))
        .collect();
    format!(
        "{}, {} {state} {thing} leaves {state} with chance {chance}: {}. {RATE_CONVERSION} 1 − e^(−({}) × {tick}).",
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

/// Returns the largest chance of a rate counted for each when it is below 1 and rounds to 1, such as "almost 1 with
/// 8".
pub fn long_tick_almost_one(count: u32) -> String {
    format!("almost 1 with {count}")
}

/// Returns the description of a rate note on how long one element stays in `state`.
///
/// `stay` is the mean stay at this tick length and `limit` the mean stay as the tick shrinks, both with their unit.
/// `rate` is the rate, and `thing` names one of what the card runs on. For example "On average a cell stays Infected
/// 10.5 days at this tick length. It approaches 1/0.1 = 10 days as the tick shrinks."
pub fn mean_stay_note(state: &str, stay: &str, rate: &str, limit: &str, thing: &str) -> String {
    format!(
        "On average {} stays {state} {stay} at this tick length. It approaches 1/{rate} = {limit} as the tick \
         shrinks.",
        with_article(thing)
    )
}

// Competing rates, the long tick and rate conversion

/// Word that starts the lower of two rate lines that compete, in place of "otherwise".
pub const COMPETING_WORD: &str = "or";

/// Returns the tooltip of the word of competing rate lines `lines`, which take elements out of `state` on a card of
/// which `thing` names one.
pub fn competing_tooltip(lines: &[usize], state: &str, thing: &str) -> String {
    format!(
        "Lines {} compete. {} leaves {state} at the sum of their rates, split between the lines in proportion to each \
         rate.",
        number_list(lines),
        upper_first(&with_article(thing))
    )
}

/// Note on a tick that is long for its rate.
pub const LONG_TICK: &str = "This tick is long for this rate. Use a shorter tick for a more accurate result.";

/// Returns the note under a rate line on a tick that is long for it, where `largest` is the chance at the largest
/// count, such as "up to 0.909 with 8".
///
/// For example "Up to 0.909 with 8. This tick is long for this rate. Use a shorter tick for a more accurate result."
pub fn long_tick_note(largest: &str) -> String {
    format!("{}. {LONG_TICK}", upper_first(largest))
}

/// Item of a line's menu that turns its chance into a rate.
pub const CHANGE_TO_RATE: &str = "Convert to a rate";

/// Item of a line's menu that turns its rate into a chance.
pub const CHANGE_TO_CHANCE: &str = "Convert to a chance";

/// Returns the note under the Time line of the sheet about the lines `lines`, which use chances per tick.
pub fn chances_per_tick_note(lines: &[usize]) -> String {
    let (word, verb) = if lines.len() == 1 {
        ("Line", "uses")
    } else {
        ("Lines", "use")
    };
    format!(
        "{word} {} {verb} chances per tick. These chances do not change with the tick length.",
        number_list(lines)
    )
}
