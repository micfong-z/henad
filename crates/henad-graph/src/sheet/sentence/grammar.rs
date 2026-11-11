//! Lists, articles, capitals, possessives and plurals, and the word for one of a kind of thing.

use crate::sheet::document::SheetDocument;

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

/// Returns `word` after its indefinite article, such as "a cell" or "an ant".
pub(super) fn with_article(word: &str) -> String {
    format!("{} {word}", indefinite_article(word))
}

/// Returns "an" before a word starting with a vowel, and "a" otherwise.
pub(super) fn indefinite_article(word: &str) -> &'static str {
    match word.chars().next().map(|first| first.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

/// Returns `words` with its first letter in lower case when it starts an ordinary capitalised word, a capital and a
/// small letter. A one-letter or all-capital word keeps its capital.
pub(crate) fn drop_ordinary_capital(words: &str) -> String {
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
pub(super) fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Returns `text` with its first letter in upper case, as a line's sentence starts.
pub(super) fn upper_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Returns `name` in the possessive: "Flockmates'" or "Next cell's".
pub(super) fn possessive(name: &str) -> String {
    if name.ends_with('s') {
        format!("{name}'")
    } else {
        format!("{name}'s")
    }
}

/// Plurals of the words that follow no rule of [`plural`], each singular in lower case with its plural.
const IRREGULAR_PLURALS: [(&str, &str); 10] = [
    ("person", "people"),
    ("child", "children"),
    ("man", "men"),
    ("woman", "women"),
    ("mouse", "mice"),
    ("foot", "feet"),
    ("thief", "thieves"),
    ("sheep", "sheep"),
    ("fish", "fish"),
    ("deer", "deer"),
];

/// Returns `name` in the plural, its last word changed by the rules of English.
///
/// A word ending in "s", "x", "z", "ch" or "sh" takes "es", a consonant and "y" turn into "ies", and "lf", "eaf",
/// "oaf" and "ife" into "lves", "eaves", "oaves" and "ives". The words of [`IRREGULAR_PLURALS`] take their own plural,
/// a capital kept. Any other word takes "s": "birds", "foxes", "flies", "wolves", "people".
pub(crate) fn plural(name: &str) -> String {
    let (head, last) = name.rsplit_once(' ').unwrap_or(("", name));
    let space = if head.is_empty() { "" } else { " " };
    let lower = last.to_lowercase();
    // The stem of `last` without its last `count` letters.
    let stem = |count: usize| {
        last.char_indices()
            .rev()
            .nth(count - 1)
            .map_or("", |(index, _)| &last[..index])
    };
    if let Some((_, irregular)) = IRREGULAR_PLURALS.iter().find(|(singular, _)| *singular == lower) {
        let word = if last.starts_with(char::is_uppercase) {
            upper_first(irregular)
        } else {
            (*irregular).to_owned()
        };
        return format!("{head}{space}{word}");
    }
    let consonant_y = lower
        .strip_suffix('y')
        .and_then(|rest| rest.chars().last())
        .is_some_and(|letter| letter.is_alphabetic() && !"aeiou".contains(letter));
    let word = if ["s", "x", "z", "ch", "sh"].iter().any(|ending| lower.ends_with(ending)) {
        format!("{last}es")
    } else if consonant_y {
        format!("{}ies", stem(1))
    } else if ["lf", "eaf", "oaf"].iter().any(|ending| lower.ends_with(ending)) {
        format!("{}ves", stem(1))
    } else if lower.ends_with("ife") {
        format!("{}ves", stem(2))
    } else {
        format!("{last}s")
    };
    format!("{head}{space}{word}")
}

/// Returns the word for one of the kind of thing `every` of `document`, as [`Sentences::thing_word`] does.
///
/// [`Sentences::thing_word`]: super::Sentences::thing_word
pub(crate) fn thing_word(document: &SheetDocument, every: &str) -> String {
    match every {
        "cells" => "cell".to_owned(),
        "ground" => "ground cell".to_owned(),
        _ => document
            .agents
            .iter()
            .find(|kind| kind.id == every)
            .map_or_else(|| every.to_owned(), |kind| drop_ordinary_capital(&kind.name)),
    }
}
