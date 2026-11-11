//! The canonical sheet writer: fixed points, the reference writer's text, hand-edited files, the number rule, the
//! generated comments and the diffs of typical edits.
//!
//! The golden files under `golden/` hold the reference writer's text for the four examples of the author format, with
//! the comment lines removed. `golden/README.md` holds the procedure that wrote them and the rules they follow.

use henad_core::metadata::TimeUnit;

use crate::sheet::document::{LineKind, ParameterType, SheetChip, SheetDocument, SheetLine, SheetParameter};
use crate::sheet::document::{SheetValue, Slot};
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::Sentences;
use crate::sheet::{ParsedSheet, SheetWriteError, format_number};

use super::sheet_document::{mutate, sheets};

/// Reads `text`, which must read, and returns its document.
fn read(text: &str) -> SheetDocument {
    ParsedSheet::parse(text)
        .unwrap_or_else(|error| panic!("the sheet does not read: {error}\n{text}"))
        .document()
        .clone()
}

/// Writes `document`, which must hold only finite numbers.
fn write(document: &SheetDocument) -> String {
    document
        .to_text()
        .unwrap_or_else(|error| panic!("the sheet does not write: {error}"))
}

/// Returns the sheet `file_name` of [`sheets`].
fn sheet_text(file_name: &str) -> &'static str {
    sheets()
        .into_iter()
        .find(|(name, _)| *name == file_name)
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("no sheet {file_name}"))
}

/// Returns `text` without its comment lines.
fn without_comments(text: &str) -> String {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect()
}

#[test]
fn every_sheet_is_a_fixed_point() {
    for (file_name, text) in sheets() {
        let written = write(&read(text));
        assert!(written == text, "{file_name} is not its canonical text:\n{written}");
    }
}

#[test]
fn the_rust_writer_agrees_with_check_py_apart_from_comments() {
    let golden = [
        ("game_of_life.toml", include_str!("golden/game_of_life.toml")),
        ("sir.toml", include_str!("golden/sir.toml")),
        ("boids.toml", include_str!("golden/boids.toml")),
        ("ants.toml", include_str!("golden/ants.toml")),
    ];
    for (file_name, expected) in golden {
        let mut document = read(sheet_text(file_name));
        // The starters take the id `sheet_<name>`, and the format's examples the name alone.
        let example_id = document.sheet.id.strip_prefix("sheet_").map(str::to_owned);
        document.sheet.id = example_id.unwrap_or_else(|| panic!("{file_name}: a starter's id starts with sheet_"));
        // The ants starter holds a note on its slower trail, which the format's example lacks.
        for card in &mut document.cards {
            for line in card.line_tables.values_mut() {
                line.note = None;
            }
        }
        let written = without_comments(&write(&document));
        assert!(
            written == expected,
            "{file_name} differs from its golden file:\n{written}"
        );
    }
}

/// Game of Life as a person might write it by hand: keys out of order, inline and standard tables where the canonical
/// text has the other, odd spellings of numbers and colours, `\r\n` line ends and comments of the author's.
const HAND_EDITED_GAME_OF_LIFE: &str = r##"# A comment of the author's, dropped on save.
kind = "sheet"
format = "henad-model"
format_version = 1

[world]
edges = "wrap"
height = 1_024
made_of = "cells"
width = 1024

[sheet]
next_line = 5
purpose = "Conway's Game of Life. Pattern to look for: gliders and other shapes that persist and travel."
name = "Game of Life"
id = "sheet_game_of_life"
version = 1

[[parameter]]
step = 1e-2
default = 3e-1
max = 1.0
min = -0.0
type = "number"
name = "Initial density"
id = "initial_density"
means = "Share of cells alive at the start"

[[cells.property]]
type = "states"
states = [{ name = "Dead", id = "dead", color = "#151515" }, { color = "#00e676", id = "alive", name = "Alive" }]
name = "Status"
id = "status"
starts_as = { otherwise = "dead", key = 2929187291, rule = "by_chance", row = [{ chance = { param = "initial_density" }, state = "alive" }] }

[color.cells]
property = "status"
rule = "by_state"

[[measure]]
state = "alive"
property = "status"
over = "cells"
rule = "count_in_state"
color = "#00e676ff"
name = "Alive"
id = "alive"

[[button]]
name = "Randomise"
id = "randomise"
does = [{ properties = ["status"], rule = "start_again" }]

[[button]]
name = "Clear"
id = "clear"

[[button.does]]
card = "clear"
rule = "run_once"

# The tick card.
[[card]]
lines = ["l1", "l2", "l3"]
runs = "each_tick"
every = "cells"
id = "cell_tick"

[card.line.l3]
condition = { right = [2.0, 3], left = { result = "l1" }, test = "not_one_of", kind = "compare" }
start_state = "alive"
new_state = "dead"
property = "status"
kind = "become"

[card.line.l1]
around = "cells_8"
name = "Alive neighbours"
kind = "count"
whose = { right = { state = "alive" }, left = { their = "status" }, test = "is", kind = "compare" }


[card.line.l2]
kind = "become"
start_state = "dead"
new_state = "alive"
property = "status"

[card.line.l2.condition]
right = +3.0
test = "is"
kind = "compare"

[card.line.l2.condition.left]
result = "l1"

[[card]]
runs = "once"
lines = ["l4"]
every = "cells"
id = "clear"

[card.line.l4]
condition.kind = "always"
start_state = "any_state"
new_state = "dead"
property = "status"
kind = "become"
"##;

#[test]
fn a_hand_edited_sheet_reads_back_to_its_canonical_text() {
    let canonical = sheet_text("game_of_life.toml");
    assert_eq!(write(&read(HAND_EDITED_GAME_OF_LIFE)), canonical);
    let crlf = HAND_EDITED_GAME_OF_LIFE.replace('\n', "\r\n");
    assert_eq!(write(&read(&crlf)), canonical);
}

/// A sheet holding values none of the committed sheets holds: empty `[cells]` and `[ground]` tables, a colour with
/// alpha, a name with a line break, a quoted line id, items that mix a literal with a chip and a phrase, branches
/// holding a phrase and a percent, a summary whose value is a phrase, and a list of a number, a percent and a chip.
const UNUSUAL_SHEET: &str = r##"format = "henad-model"
format_version = 1
kind = "sheet"

[sheet]
version = 1
id = "unusual"
name = "Two\nlines"
next_line = 9

[world]
made_of = "agents"
width = 10
height = 10
edges = "walls"

[cells]

[ground]

[[agents]]
id = "ant"
name = "Ant"
count = 1
max_count = 2

[[agents.property]]
id = "cargo"
name = "Cargo"
type = "states"
states = [{ id = "a", name = "A", color = "#11223344" }, { id = "b", name = "B", color = "#AABBCC" }]

[[parameter]]
id = "p"
name = "P"
type = "number"
min = -0.0
max = 1e300
default = 2.5e-7
step = 0.0001

[[card]]
id = "tick"
every = "ant"
runs = "each_tick"
lines = ["l1", "l2", "l 3"]

[card.line.l1]
kind = "any_of"
name = "Mixed\tname"
items = [true, { result = "l2" }, { kind = "chance", chance = { percent = 30 } }]

[card.line.l2]
kind = "depending_on"
name = "Way"
property = "cargo"
branches = { z = { percent = 12.5 }, b = { kind = "calculate", formula = "1 + 2" }, a = 3 }

[card.line."l 3"]
kind = "look_around"
name = "Look"
around = "agents_within"
agent_kind = "ant"
summaries = [{ id = "s", name = "s", summary = "average", value = { kind = "calculate", formula = "my.x * 2" } }]
right = [1, { percent = 5 }, { state = "a" }]
"##;

#[test]
fn unusual_values_read_back_to_the_same_document() {
    let document = read(UNUSUAL_SHEET);
    let written = write(&document);
    assert_eq!(read(&written), document, "{written}");
    assert_eq!(write(&read(&written)), written);
    for expected in [
        "[cells]\n",
        "[ground]\n",
        "color = \"#11223344\"",
        "[card.line.\"l 3\"]",
        "[[card.line.\"l 3\".summaries]]",
        "items = [true, { result = \"l2\" }, { kind = \"chance\", chance = { percent = 30 } }]",
        "branches = { a = 3, b = { kind = \"calculate\", formula = \"1 + 2\" }, z = { percent = 12.5 } }",
        "right = [1, { percent = 5 }, { state = \"a\" }]",
        "min = 0\nmax = 1e300\ndefault = 2.5e-7\nstep = 0.0001\n",
        "# Mixed name = any of yes, Way, with chance 30%\n",
    ] {
        assert!(written.contains(expected), "{expected:?} is not in\n{written}");
    }
}

#[test]
fn a_number_that_is_not_finite_is_refused() {
    let mut document = read(sheet_text("game_of_life.toml"));
    document.parameters[0].default = f64::NAN;
    let error = document.to_text().expect_err("NaN writes");
    assert_eq!(error.key, "default");
    assert!(error.to_string().contains("only finite numbers"), "{error}");
    let mut document = read(sheet_text("game_of_life.toml"));
    if let Some((_, line)) = document.cards[0].line_tables.iter_mut().nth(1)
        && let Some(SheetValue::Phrase(condition)) = match &mut line.condition {
            Slot::Filled(value) => Some(value),
            Slot::Blank => None,
        }
    {
        condition.right = Slot::Filled(SheetValue::Literal(crate::sheet::document::SheetLiteral::Number(
            f64::INFINITY,
        )));
    }
    let error: SheetWriteError = document.to_text().expect_err("an infinity writes");
    assert_eq!(error.key, "right");
}

#[test]
fn numbers_follow_the_sheet_number_rule() {
    for (value, text) in [
        (1024.0, "1024"),
        (1.0, "1"),
        (0.0, "0"),
        (-0.0, "0"),
        (-3.0, "-3"),
        (0.3, "0.3"),
        (0.1 + 0.2, "0.30000000000000004"),
        (0.0005, "0.0005"),
        (0.0001, "0.0001"),
        (1e-5, "1e-5"),
        (2.5e-7, "2.5e-7"),
        (-2.5e-7, "-2.5e-7"),
        (12.5, "12.5"),
        (1e15 + 0.5, "1000000000000000.5"),
        (9_007_199_254_740_992.0, "9007199254740992.0"),
        (1e16, "1e16"),
        (1.5e16, "1.5e16"),
        (1e300, "1e300"),
    ] {
        assert_eq!(format_number(value).as_deref(), Some(text), "{value:e}");
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(format_number(value), None, "{value}");
    }
}

#[test]
fn every_line_sentence_equals_its_comment() {
    for (file_name, text) in sheets() {
        let document = read(text);
        let sentences = Sentences::new(&document);
        let lines: Vec<&str> = text.lines().collect();
        let mut card = 0;
        let mut checked = 0;
        for (index, line) in lines.iter().enumerate() {
            let comment = index.checked_sub(1).and_then(|before| lines[before].strip_prefix("# "));
            if *line == "[[card]]" {
                let expected = sentences.card_heading(&document.cards[card]);
                assert_eq!(comment, Some(expected.as_str()), "{file_name}: card {card}");
                card += 1;
            } else if let Some(id) = line
                .strip_prefix("[card.line.")
                .and_then(|rest| rest.strip_suffix(']'))
                .filter(|id| !id.contains('.'))
            {
                let relations = CardRelations::new(&document, &document.cards[card - 1]);
                let expected = sentences.line(id, &relations.otherwise_numbers(id));
                assert_eq!(comment, Some(expected.as_str()), "{file_name}: {id}");
                checked += 1;
            }
        }
        let total: usize = document.cards.iter().map(|card| card.line_tables.len()).sum();
        assert_eq!(checked, total, "{file_name}");
    }
}

/// Returns the hunks that turn the lines of `old` into those of `new`, each as its removed and its added lines, from a
/// longest common subsequence of lines.
fn hunks(old: &str, new: &str) -> Vec<(Vec<String>, Vec<String>)> {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    // `common[i][j]` is the length of the longest common subsequence of `old[i..]` and `new[j..]`.
    let mut common = vec![vec![0_usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            common[i][j] = if old[i] == new[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let mut out: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    let mut current: Option<(Vec<String>, Vec<String>)> = None;
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            out.extend(current.take());
            i += 1;
            j += 1;
        } else if j < new.len() && (i == old.len() || common[i][j + 1] >= common[i + 1][j]) {
            current.get_or_insert_with(Default::default).1.push(new[j].to_owned());
            j += 1;
        } else {
            current.get_or_insert_with(Default::default).0.push(old[i].to_owned());
            i += 1;
        }
    }
    out.extend(current);
    out
}

/// Returns `lines` without the blank lines at either end.
fn trimmed(lines: &[String]) -> Vec<String> {
    let start = lines.iter().position(|line| !line.is_empty()).unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(start, |end| end + 1);
    lines[start..end].to_vec()
}

/// Checks that `old` turns into `new` through exactly `expected`, each hunk's removed and added lines, blank lines at
/// a hunk's ends aside.
pub(super) fn assert_hunks(old: &str, new: &str, expected: &[(&[&str], &[&str])]) {
    let found = hunks(old, new);
    let found: Vec<(Vec<String>, Vec<String>)> = found
        .iter()
        .map(|(removed, added)| (trimmed(removed), trimmed(added)))
        .collect();
    let expected: Vec<(Vec<String>, Vec<String>)> = expected
        .iter()
        .map(|(removed, added)| {
            (
                removed.iter().map(|line| (*line).to_owned()).collect(),
                added.iter().map(|line| (*line).to_owned()).collect(),
            )
        })
        .collect();
    assert_eq!(found, expected, "the diff of\n{new}");
}

#[test]
fn a_new_waning_line_changes_four_hunks() {
    let old = sheet_text("sir.toml");
    let mut document = read(old);
    document.sheet.next_line = 6;
    let mut waning = SheetParameter::new("waning_rate", "Waning rate", ParameterType::Number, 0.0, 1.0, 0.01);
    waning.step = Some(0.001);
    waning.unit = Some("per day".to_owned());
    waning.means = Some("Rate at which a recovered cell loses its immunity".to_owned());
    document.parameters.push(waning);
    let mut rate = SheetLine::new(LineKind::Rate);
    rate.rate = Slot::Filled(SheetValue::Chip(SheetChip::Param("waning_rate".to_owned())));
    rate.rate_unit = Some(TimeUnit::Day);
    let mut line = SheetLine::new(LineKind::Become);
    line.key = Some(1_585_830_067);
    line.property = Some("health".to_owned());
    line.new_state = Some("susceptible".to_owned());
    line.start_state = Some("recovered".to_owned());
    line.condition = Slot::Filled(SheetValue::Phrase(Box::new(rate)));
    document.cards[0].lines.push("l5".to_owned());
    document.cards[0].line_tables.insert("l5".to_owned(), line);
    let new = write(&document);
    assert_hunks(
        old,
        &new,
        &[
            (&["next_line = 5"], &["next_line = 6"]),
            (
                &[],
                &[
                    "[[parameter]]",
                    "id = \"waning_rate\"",
                    "name = \"Waning rate\"",
                    "type = \"number\"",
                    "min = 0",
                    "max = 1",
                    "default = 0.01",
                    "step = 0.001",
                    "unit = \"per day\"",
                    "means = \"Rate at which a recovered cell loses its immunity\"",
                ],
            ),
            (
                &["lines = [\"l1\", \"l2\", \"l3\"]"],
                &["lines = [\"l1\", \"l2\", \"l3\", \"l5\"]"],
            ),
            (
                &[],
                &[
                    "# Become Susceptible from Recovered, at rate Waning rate per day",
                    "[card.line.l5]",
                    "kind = \"become\"",
                    "key = 1585830067",
                    "property = \"health\"",
                    "new_state = \"susceptible\"",
                    "start_state = \"recovered\"",
                    "",
                    "[card.line.l5.condition]",
                    "kind = \"rate\"",
                    "rate = { param = \"waning_rate\" }",
                    "rate_unit = \"day\"",
                ],
            ),
        ],
    );
    assert_eq!(read(&new), document);
}

#[test]
fn a_steer_reorder_changes_one_line() {
    let old = sheet_text("boids.toml");
    let mut document = read(old);
    let lines = &mut document.cards[0].lines;
    let cohesion = lines.remove(4);
    lines.insert(2, cohesion);
    let new = write(&document);
    assert_hunks(
        old,
        &new,
        &[(
            &["lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\"]"],
            &["lines = [\"l1\", \"l2\", \"l5\", \"l3\", \"l4\", \"l6\", \"l7\"]"],
        )],
    );
}

#[test]
fn a_parameter_rename_changes_its_name_and_the_comment_that_shows_it() {
    let old = sheet_text("boids.toml");
    let mut document = read(old);
    let cohesion = document
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == "cohesion")
        .unwrap_or_else(|| panic!("no cohesion"));
    cohesion.name = "Pull to centre".to_owned();
    let new = write(&document);
    assert_hunks(
        old,
        &new,
        &[
            (&["name = \"Cohesion\""], &["name = \"Pull to centre\""]),
            (
                &["# Steer Velocity by Flockmates' centre, strength Cohesion"],
                &["# Steer Velocity by Flockmates' centre, strength Pull to centre"],
            ),
        ],
    );
}

#[test]
fn every_readable_mutation_writes_text_that_reads_back() {
    let mut rng = 0x7772_6974_6500_0001u64;
    let mut written = 0usize;
    for (file_name, text) in sheets() {
        for edit in 0..300 {
            let mut edited = mutate(text, &mut rng);
            if edit % 3 == 0 {
                edited = mutate(&edited, &mut rng);
            }
            let Ok(parsed) = ParsedSheet::parse(&edited) else {
                continue;
            };
            let text = parsed
                .document()
                .to_text()
                .unwrap_or_else(|error| panic!("{file_name}: a read sheet does not write: {error}\n{edited}"));
            let again = ParsedSheet::parse(&text)
                .unwrap_or_else(|error| panic!("{file_name}: the written text does not read: {error}\n{text}"));
            assert_eq!(
                again.document(),
                parsed.document(),
                "{file_name}:\n{edited}\n---\n{text}"
            );
            assert_eq!(write(again.document()), text, "{file_name}");
            written += 1;
        }
    }
    assert!(written > 500, "only {written} edits read");
}
