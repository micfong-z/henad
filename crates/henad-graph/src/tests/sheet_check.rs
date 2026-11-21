//! Tests of the sheet checker on whole sheets and of the relations between lines: the "otherwise" rule, competing
//! groups, skipped lines and their notes.

use henad_core::diagnostic::DiagnosticSeverity;

use super::sheet_support::{edited_sheet, read, sheet_text, sheets};
use crate::sheet::check::{self, SheetReport};
use crate::sheet::document::{LineKind, SheetDocument, SheetLine, SheetValue, Slot};
use crate::sheet::relations::{CardRelations, RelationNote, RelationNoteKind};
use crate::sheet::sentence::{self, Blank, LineSelect, NOT_RUN_TICK, ONE_AGENT_KIND, SHEET_EMPTY, Sentences};
use crate::sheet::{ParsedSheet, starters};

/// Returns the report of checking `text`, which must read.
fn report(text: &str) -> SheetReport {
    ParsedSheet::parse(text)
        .unwrap_or_else(|error| panic!("the sheet does not read: {error}"))
        .check()
}

/// Returns the sentence of line `line_id` of card `card` of `document`, with the "otherwise" words of the relations.
fn sentence_of(document: &SheetDocument, card: usize, line_id: &str) -> String {
    let relations = CardRelations::new(document, &document.cards[card]);
    Sentences::new(document).line(line_id, &relations.otherwise_numbers(line_id))
}

/// Returns the "otherwise" partners of line `line_id` of card `card` of `document`, by line number.
fn partners(document: &SheetDocument, card: usize, line_id: &str) -> Vec<usize> {
    CardRelations::new(document, &document.cards[card]).otherwise_numbers(line_id)
}

/// Returns the line `line_id` of `document` to edit.
fn line_mut<'a>(document: &'a mut SheetDocument, line_id: &str) -> &'a mut SheetLine {
    document
        .cards
        .iter_mut()
        .find_map(|card| card.line_tables.get_mut(line_id))
        .unwrap_or_else(|| panic!("no line {line_id}"))
}

#[test]
fn every_sheet_checks_clean() {
    for (file_name, text) in sheets() {
        let sheet = ParsedSheet::parse(text).unwrap_or_else(|error| panic!("{file_name} does not read: {error}"));
        let placed = sheet.check();
        let messages: Vec<String> = placed
            .diagnostics
            .iter()
            .map(|placed| placed.diagnostic.to_string())
            .collect();
        assert!(messages.is_empty(), "{file_name}:\n{}", messages.join("\n"));
        assert!(check::check(sheet.document()).diagnostics.is_empty(), "{file_name}");
    }
}

#[test]
fn the_report_is_in_file_order_with_each_line_problem_at_its_table() {
    // l3 sits below l1 in the file and above it on the card.
    let text = edited_sheet(
        "game_of_life.toml",
        &[
            ("lines = [\"l1\", \"l2\", \"l3\"]", "lines = [\"l3\", \"l2\", \"l1\"]"),
            ("default = 0.3", "default = 2"),
        ],
    );
    let report = report(&text);
    let starts: Vec<usize> = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.diagnostic.span.as_ref().map_or(0, |span| span.start))
        .collect();
    assert!(starts.is_sorted(), "{starts:?}");
    let lines: Vec<Option<&str>> = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.line.as_deref())
        .collect();
    assert_eq!(lines, [None, Some("l2"), Some("l3")]);
    assert_eq!(report.diagnostics[0].diagnostic.severity, DiagnosticSeverity::Error);
    assert!(report.has_errors());
    assert_eq!(report.skipped().collect::<Vec<_>>(), ["l2", "l3"]);
    assert_eq!(report.warnings().count(), 2);
    for diagnostic in &report.diagnostics[1..] {
        assert_eq!(diagnostic.card, Some(0));
        let span = diagnostic.diagnostic.span.clone().expect("a line problem has a place");
        let header = format!("[card.line.{}]", diagnostic.line.as_deref().unwrap_or(""));
        assert_eq!(&text[span], header);
    }
}

/// Returns Game of Life with a result line l5 at line 2, both Become lines from Dead, line 3 testing Alive neighbours
/// with `second` and line 4 with `third`, each a `(test, right)` pair of a compare.
fn two_tests(second: (&str, &str), third: (&str, &str)) -> SheetDocument {
    let text = edited_sheet(
        "game_of_life.toml",
        &[
            ("next_line = 5", "next_line = 6"),
            (
                "lines = [\"l1\", \"l2\", \"l3\"]",
                "lines = [\"l1\", \"l5\", \"l2\", \"l3\"]",
            ),
            (
                "new_state = \"dead\"\nstart_state = \"alive\"",
                "new_state = \"dead\"\nstart_state = \"dead\"",
            ),
            (
                "test = \"is\"\nleft = { result = \"l1\" }\nright = 3",
                &format!(
                    "test = \"{}\"\nleft = {{ result = \"l1\" }}\nright = {}",
                    second.0, second.1
                ),
            ),
            (
                "test = \"not_one_of\"\nleft = { result = \"l1\" }\nright = [2, 3]",
                &format!(
                    "test = \"{}\"\nleft = {{ result = \"l1\" }}\nright = {}",
                    third.0, third.1
                ),
            ),
            (
                "# Lines of Clear",
                "[card.line.l5]\nkind = \"compare\"\nname = \"Crowded\"\ntest = \"more_than\"\nleft = { result = \"l1\" \
                 }\nright = 3\n\n# Lines of Clear",
            ),
        ],
    );
    let report = report(&text);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    read(&text)
}

#[test]
fn otherwise_names_its_partner_and_skips_exclusive_tests() {
    let exclusive = [
        (("is", "3"), ("not_one_of", "[2, 3]"), "is none of the numbers 2, 3"),
        (("at_most", "4"), ("at_least", "5"), "is at least 5"),
        (("less_than", "2"), ("more_than", "3"), "is more than 3"),
        (("one_of", "[2, 3]"), ("is", "4"), "is 4"),
    ];
    for (second, third, words) in exclusive {
        let game = two_tests(second, third);
        assert!(partners(&game, 0, "l3").is_empty(), "{second:?} against {third:?}");
        assert!(!CardRelations::new(&game, &game.cards[0]).can_both_apply("l2", "l3"));
        assert_eq!(
            sentence_of(&game, 0, "l3"),
            format!("Become Dead from Dead, when Alive neighbours {words}")
        );
    }
    let overlapping = [
        (("is", "3"), ("at_least", "3"), "is at least 3"),
        (("is", "3"), ("one_of", "[2, 3]"), "is one of the numbers 2, 3"),
        (("at_most", "4"), ("at_least", "4"), "is at least 4"),
        (("is_not", "3"), ("is_not", "4"), "is not 4"),
    ];
    for (second, third, words) in overlapping {
        let game = two_tests(second, third);
        assert_eq!(partners(&game, 0, "l3"), [3], "{second:?} against {third:?}");
        assert!(CardRelations::new(&game, &game.cards[0]).can_both_apply("l2", "l3"));
        assert_eq!(
            sentence_of(&game, 0, "l3"),
            format!("otherwise (when line 3 does not apply) become Dead from Dead, when Alive neighbours {words}")
        );
    }
}

#[test]
fn the_starters_read_otherwise_as_the_final_words_give_them() {
    let game = read(sheet_text("game_of_life.toml"));
    for id in ["l1", "l2", "l3"] {
        assert!(partners(&game, 0, id).is_empty(), "{id}");
    }
    let relations = CardRelations::new(&game, &game.cards[0]);
    assert_eq!(relations.order(), ["l1", "l2", "l3"]);
    assert_eq!(relations.number("l3"), Some(3));
    assert!(!relations.can_both_apply("l2", "l3"));
    let vote = read(sheet_text("vote.toml"));
    assert_eq!(partners(&vote, 0, "l3"), [2]);
    assert_eq!(
        sentence_of(&vote, 0, "l3"),
        "otherwise (when line 2 does not apply) become A from any state, always"
    );
}

#[test]
fn the_forest_fire_lightning_line_reads_otherwise_against_the_spread_line() {
    let fire = read(sheet_text("forest_fire.toml"));
    assert_eq!(partners(&fire, 0, "l3"), [2]);
    assert_eq!(
        sentence_of(&fire, 0, "l3"),
        "otherwise (when line 2 does not apply) become Burning from Tree, with chance Lightning chance"
    );
    for id in ["l2", "l4", "l5"] {
        assert!(partners(&fire, 0, id).is_empty(), "{id}");
    }
}

#[test]
fn the_ants_set_lines_follow_the_recorded_set_rule() {
    let ants = read(sheet_text("ants.toml"));
    // The When lines around lines 11 and 14 test my Cargo against Searching and Carrying food, each inside an all of.
    assert!(partners(&ants, 0, "l14").is_empty());
    assert_eq!(partners(&ants, 0, "l16"), [11, 14]);
    assert_eq!(
        sentence_of(&ants, 0, "l16"),
        "otherwise (when lines 11 and 14 do not apply) set Reward held to 0, always"
    );
    // The two Become lines from any state sit in the same two When lines.
    assert!(partners(&ants, 0, "l13").is_empty());
    let relations = CardRelations::new(&ants, &ants.cards[0]);
    assert_eq!(relations.parent("l14"), Some("l12"));
    assert!(relations.is_within("l14", "l12"));
    assert!(!relations.is_within("l16", "l12"));
    // An any of tests nothing, so it no longer keeps line 14 apart from line 11.
    let mut changed = ants.clone();
    if let Some(SheetValue::Phrase(phrase)) = match &mut line_mut(&mut changed, "l12").condition {
        Slot::Filled(value) => Some(value),
        Slot::Blank => None,
    } {
        phrase.kind = LineKind::AnyOf;
    }
    assert_eq!(partners(&changed, 0, "l14"), [11]);
}

/// Returns SIR with a Dead state and line 2 leaving Infected for Dead, with `condition` as its condition table.
fn two_exits(condition: &str) -> SheetDocument {
    let text = edited_sheet(
        "sir.toml",
        &[
            (
                "  { id = \"recovered\", name = \"Recovered\", color = \"#808080\" },\n",
                "  { id = \"recovered\", name = \"Recovered\", color = \"#808080\" },\n  { id = \"dead\", name = \"Dead\", \
                 color = \"#000000\" },\n",
            ),
            (
                "new_state = \"infected\"\nstart_state = \"susceptible\"\n\n[card.line.l2.condition]\nkind = \"rate\"\nrate \
                 = { param = \"infection_rate\" }\nrate_unit = \"day\"\ntries = { result = \"l1\" }",
                &format!("new_state = \"dead\"\nstart_state = \"infected\"\n\n[card.line.l2.condition]\n{condition}"),
            ),
        ],
    );
    let report = report(&text);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    read(&text)
}

#[test]
fn two_rate_exits_form_one_group() {
    let sir = two_exits("kind = \"rate\"\nrate = { param = \"infection_rate\" }\nrate_unit = \"day\"");
    let relations = CardRelations::new(&sir, &sir.cards[0]);
    assert_eq!(relations.competing_groups(), [vec!["l2", "l3"]]);
    assert!(relations.otherwise_of("l3").is_empty());
    assert!(relations.can_both_apply("l2", "l3"));
    // The committed SIR leaves two different states, so it has no group.
    let committed = read(sheet_text("sir.toml"));
    assert!(
        CardRelations::new(&committed, &committed.cards[0])
            .competing_groups()
            .is_empty()
    );
}

#[test]
fn a_chance_line_beside_a_rate_line_keeps_otherwise() {
    let sir = two_exits("kind = \"chance\"\nchance = 0.1");
    let relations = CardRelations::new(&sir, &sir.cards[0]);
    assert!(relations.competing_groups().is_empty());
    assert_eq!(relations.otherwise_of("l3"), ["l2"]);
    assert_eq!(
        sentence_of(&sir, 0, "l3"),
        "otherwise (when line 2 does not apply) become Recovered from Infected, at rate Recovery rate per day"
    );
}

#[test]
fn a_line_reading_a_skipped_line_is_skipped() {
    let text = edited_sheet(
        "game_of_life.toml",
        &[(
            "left = { their = \"status\" }\nright = { state = \"alive\" }\n",
            "left = { their = \"status\" }\n",
        )],
    );
    let report = report(&text);
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.skip_note("l1"), Some("Not run yet: choose a state"));
    for id in ["l2", "l3"] {
        assert_eq!(
            report.skip_note(id),
            Some("Not run yet: Alive neighbours on line 1 is not finished"),
            "{id}"
        );
    }
    assert!(!report.is_skipped("l4"));
}

#[test]
fn a_line_inside_a_skipped_when_is_skipped() {
    let mut ants = read(sheet_text("ants.toml"));
    line_mut(&mut ants, "l12").condition = Slot::Blank;
    let report = check::check(&ants);
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.skip_note("l12"), Some("Not run yet: choose when"));
    for id in ["l13", "l14", "l15"] {
        assert_eq!(
            report.skip_note(id),
            Some("Not run yet: line 12 is not finished"),
            "{id}"
        );
    }
    assert!(!report.is_skipped("l16"));
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.diagnostic.span.is_none())
    );
}

#[test]
fn every_line_inside_a_skipped_repeat_is_skipped_at_any_depth() {
    // Repeat l1 with no times holds When l3, which holds the result line l4, then the Move l2.
    let mut text = edited_sheet(
        "random_walk.toml",
        &[
            ("next_line = 3", "next_line = 5"),
            (
                "times = { param = \"steps_per_tick\" }\nlines = [\"l2\"]",
                "lines = [\"l3\", \"l2\"]",
            ),
        ],
    );
    text.push_str(
        "\n[card.line.l3]\nkind = \"when\"\nlines = [\"l4\"]\n\n[card.line.l3.condition]\nkind = \"compare\"\ntest = \
         \"more_than\"\nleft = { param = \"step_size\" }\nright = 1\n\n[card.line.l4]\nkind = \"calculate\"\nname = \
         \"Double step\"\nformula = \"step_size * 2\"\n",
    );
    let report = report(&text);
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(
        report.skip_note("l1"),
        Some(sentence::not_run_choose(Blank::Times.want()).as_str())
    );
    let unfinished = sentence::not_run_when_unfinished(1);
    for id in ["l3", "l4", "l2"] {
        assert_eq!(report.skip_note(id), Some(unfinished.as_str()), "{id}");
    }
    // The report is in file order, and l2's table comes before the new tables.
    assert_eq!(report.skipped().collect::<Vec<_>>(), ["l1", "l2", "l3", "l4"]);
    // A result inside a skipped When still runs once its Repeat runs.
    let when_only = text
        .replace("lines = [\"l3\", \"l2\"]", "times = 2\nlines = [\"l3\", \"l2\"]")
        .replace("left = { param = \"step_size\" }\n", "");
    let report = self::report(&when_only);
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.skipped().collect::<Vec<_>>(), ["l3"]);
}

#[test]
fn a_count_part_or_summary_is_tried_at_whole_numbers() {
    // More than 2 and less than 3 hold together only between whole numbers.
    let text = edited_sheet(
        "boids.toml",
        &[
            (
                "summary = \"total\", value = { relation = \"offset_away_from_them\" } },",
                "summary = \"total\", value = { relation = \"offset_away_from_them\" } },\n  { id = \"near\", name = \
                 \"near\", summary = \"count\" },",
            ),
            (
                "strength = { param = \"separation\" }",
                "strength = { param = \"separation\" }\ncondition = { kind = \"compare\", test = \"more_than\", left = { \
                 result = \"l2\", part = \"count\" }, right = 2 }",
            ),
            (
                "strength = { param = \"alignment\" }",
                "strength = { param = \"alignment\" }\ncondition = { kind = \"compare\", test = \"less_than\", left = { \
                 result = \"l2\", part = \"count\" }, right = 3 }",
            ),
        ],
    );
    let boids = read(&text);
    assert!(
        check::check(&boids).diagnostics.is_empty(),
        "{:?}",
        check::check(&boids).diagnostics
    );
    let relations = |document: &SheetDocument| {
        let card = document
            .cards
            .iter()
            .position(|card| card.line_tables.contains_key("l3"));
        card.map(|card| CardRelations::new(document, &document.cards[card]).can_both_apply("l3", "l4"))
    };
    assert_eq!(relations(&boids), Some(false));
    let summary = read(&text.replace("part = \"count\"", "part = \"near\""));
    assert_eq!(relations(&summary), Some(false));
    // A total is no count, and 2.5 passes both tests.
    let total = read(&text.replace("part = \"count\"", "part = \"push\""));
    assert_eq!(relations(&total), Some(true));
}

/// Returns Game of Life with lines 1 and 2 inside a When on line 1, so that line 3 uses a result from inside it.
fn scoped_game() -> String {
    edited_sheet(
        "game_of_life.toml",
        &[
            ("next_line = 5", "next_line = 6"),
            ("lines = [\"l1\", \"l2\", \"l3\"]", "lines = [\"l5\", \"l3\"]"),
            (
                "# Lines of Clear",
                "[card.line.l5]\nkind = \"when\"\nlines = [\"l1\", \"l2\"]\n\n[card.line.l5.condition]\nkind = \
                 \"compare\"\ntest = \"is\"\nleft = { my = \"status\" }\nright = { state = \"dead\" }\n\n# Lines of Clear",
            ),
        ],
    )
}

#[test]
fn a_result_inside_a_when_is_open_only_to_the_lines_inside_it() {
    let report = report(&scoped_game());
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert!(!report.is_skipped("l2"));
    assert_eq!(
        report.skip_note("l3"),
        Some("Uses Alive neighbours from inside line 1. Only lines inside line 1 can use it. Choose another value.")
    );
}

#[test]
#[expect(clippy::too_many_lines, reason = "one case per kind of skip reads best as one table")]
fn every_skip_note_equals_its_wording_table_entry() {
    let blank = |old: &str, new: &str| report(&edited_sheet("game_of_life.toml", &[(old, new)]));
    // A blank, in the words of what it wants.
    let cases = [
        (
            blank("right = { state = \"alive\" }\n", ""),
            "l1",
            "Not run yet: choose a state",
            sentence::not_run_choose(Blank::State.want()),
        ),
        (
            blank("start_state = \"dead\"\n", ""),
            "l2",
            "Not run yet: choose the old state",
            sentence::not_run_choose(Blank::OldState.want()),
        ),
        (
            blank("around = \"cells_8\"\n", ""),
            "l1",
            "Not run yet: choose which cells",
            sentence::not_run_choose(LineSelect::WhichCells.words()),
        ),
        (
            blank(
                "test = \"is\"\nleft = { result = \"l1\" }\n",
                "left = { result = \"l1\" }\n",
            ),
            "l2",
            "Not run yet: choose comparison",
            sentence::not_run_choose(LineSelect::Comparison.words()),
        ),
        (
            blank("left = { result = \"l1\" }\nright = 3", "left = { result = \"l1\" }"),
            "l2",
            "Not run yet: choose a number",
            sentence::not_run_choose(Blank::Number.want()),
        ),
        (
            blank("right = [2, 3]", ""),
            "l3",
            "Not run yet: choose a list of numbers",
            sentence::not_run_choose(Blank::NumberList.want()),
        ),
        (
            blank(
                "start_state = \"any_state\"\n\n[card.line.l4.condition]\nkind = \"always\"",
                "start_state = \"any_state\"",
            ),
            "l4",
            "Not run yet: choose when",
            sentence::not_run_choose(Blank::When.want()),
        ),
        (
            blank(
                "test = \"is\"\nleft = { result = \"l1\" }\nright = 3",
                "test = \"outside\"\nleft = { result = \"l1\" }\nlow = 2",
            ),
            "l2",
            "Not run yet: choose the upper number",
            sentence::not_run_choose(Blank::UpperNumber.want()),
        ),
        (
            report(&edited_sheet(
                "sir.toml",
                &[(
                    "rate = { param = \"recovery_rate\" }\nrate_unit = \"day\"",
                    "rate = { param = \"recovery_rate\" }",
                )],
            )),
            "l3",
            "Not run yet: choose unit of time",
            sentence::not_run_choose(LineSelect::UnitOfTime.words()),
        ),
        // A result of a deleted line reads as a blank.
        (
            blank(
                "left = { result = \"l1\" }\nright = 3",
                "left = { result = \"l9\" }\nright = 3",
            ),
            "l2",
            "Not run yet: choose a value",
            sentence::not_run_choose(Blank::Value.want()),
        ),
        // A result below, a result from inside a When, a skipped result, a skipped When and a missing tick length.
        (
            blank("lines = [\"l1\", \"l2\", \"l3\"]", "lines = [\"l2\", \"l1\", \"l3\"]"),
            "l2",
            "Uses Alive neighbours, which is below this line",
            sentence::uses_result_below("Alive neighbours"),
        ),
        (
            report(&scoped_game()),
            "l3",
            "Uses Alive neighbours from inside line 1. Only lines inside line 1 can use it. Choose another value.",
            sentence::uses_result_inside("Alive neighbours", 1),
        ),
        (
            blank("right = { state = \"alive\" }\n", ""),
            "l2",
            "Not run yet: Alive neighbours on line 1 is not finished",
            sentence::not_run_result_unfinished("Alive neighbours", 1),
        ),
        (
            report(&edited_sheet(
                "sir.toml",
                &[("[time]\ntick_length = 1\ntick_unit = \"day\"\n\n", "")],
            )),
            "l3",
            "Not run yet: set how long one tick is, under World on the sheet",
            NOT_RUN_TICK.to_owned(),
        ),
    ];
    for (report, line, words, entry) in cases {
        assert!(!report.has_errors(), "{line}: {:?}", report.diagnostics);
        assert_eq!(words, entry, "the wording table changed");
        assert_eq!(report.skip_note(line), Some(words), "{line}");
    }
    let mut ants = read(sheet_text("ants.toml"));
    line_mut(&mut ants, "l9").condition = Slot::Blank;
    assert_eq!(
        check::check(&ants).skip_note("l10"),
        Some(sentence::not_run_when_unfinished(9).as_str())
    );
    assert_eq!(
        sentence::not_run_when_unfinished(9),
        "Not run yet: line 9 is not finished"
    );
}

#[test]
fn the_outside_test_stands_only_in_a_condition_and_counts_for_otherwise() {
    let game = two_tests(("less_than", "2"), ("is", "3"));
    let mut outside = game.clone();
    if let Some(SheetValue::Phrase(phrase)) = match &mut line_mut(&mut outside, "l2").condition {
        Slot::Filled(value) => Some(value),
        Slot::Blank => None,
    } {
        phrase.test = Some(crate::sheet::document::CompareTest::Outside);
        phrase.right = Slot::Blank;
        phrase.low = Slot::Filled(SheetValue::Literal(crate::sheet::document::SheetLiteral::Number(2.0)));
        phrase.high = Slot::Filled(SheetValue::Literal(crate::sheet::document::SheetLiteral::Number(4.0)));
    }
    assert!(check::check(&outside).diagnostics.is_empty());
    // Less than 2 or more than 4 never holds at 3.
    assert!(partners(&outside, 0, "l3").is_empty());
    let mut compare_line = outside.clone();
    let l5 = line_mut(&mut compare_line, "l5");
    l5.test = Some(crate::sheet::document::CompareTest::Outside);
    l5.low = Slot::Filled(SheetValue::Literal(crate::sheet::document::SheetLiteral::Number(1.0)));
    l5.high = Slot::Filled(SheetValue::Literal(crate::sheet::document::SheetLiteral::Number(5.0)));
    let messages: Vec<String> = check::check(&compare_line)
        .errors()
        .map(|diagnostic| diagnostic.diagnostic.message.clone())
        .collect();
    assert_eq!(
        messages,
        ["is less than ... or more than ... can only be used in a condition"]
    );
}

#[test]
fn a_when_that_is_always_and_the_reserved_ids_are_errors() {
    let mut ants = read(sheet_text("ants.toml"));
    line_mut(&mut ants, "l9").condition = line_mut(&mut ants.clone(), "l10").condition.clone();
    let errors = |document: &SheetDocument| -> Vec<String> {
        check::check(document)
            .errors()
            .map(|diagnostic| diagnostic.diagnostic.message.clone())
            .collect()
    };
    assert_eq!(errors(&ants), ["a When cannot be always"]);
    for (old, new, message) in [
        (
            "id = \"initial_density\"",
            "id = \"tick_length\"",
            "the parameter id 'tick_length' is reserved",
        ),
        (
            "id = \"initial_density\"",
            "id = \"henad_density\"",
            "the parameter id 'henad_density' is reserved",
        ),
        (
            "id = \"initial_density\"",
            "id = \"grid_width\"",
            "the parameter id 'grid_width' is reserved",
        ),
        (
            "id = \"initial_density\"",
            "id = \"amount\"",
            "the parameter id 'amount' is reserved",
        ),
        (
            "id = \"alive\", name",
            "id = \"any_state\", name",
            "the state id 'any_state' is reserved",
        ),
        (
            "id = \"status\"",
            "id = \"tick_length\"",
            "the property id 'tick_length' is reserved",
        ),
        (
            "id = \"clear\"\nevery",
            "id = \"the_id_of_this_card_holds_forty_nine_bytes_in_all\"\nevery",
            "the card id 'the_id_of_this_card_holds_forty_nine_bytes_in_all' holds 49 bytes, expected at most 48",
        ),
    ] {
        let text = sheet_text("game_of_life.toml").replacen(old, new, 1);
        let messages: Vec<String> = report(&text)
            .errors()
            .map(|error| error.diagnostic.message.clone())
            .collect();
        assert!(messages.iter().any(|found| found == message), "{message}: {messages:?}");
    }
}

#[test]
fn a_second_agent_kind_is_refused() {
    let mut boids = read(sheet_text("boids.toml"));
    let mut second = boids.agents[0].clone();
    second.id = "hawk".to_owned();
    for property in &mut second.properties {
        property.id = format!("hawk_{}", property.id);
        if let Some(start) = &mut property.starts_as {
            start.key = start.key.map(|key| key ^ 1);
        }
    }
    boids.agents.push(second);
    let errors: Vec<String> = check::check(&boids)
        .errors()
        .map(|diagnostic| diagnostic.diagnostic.message.clone())
        .collect();
    assert_eq!(errors, [ONE_AGENT_KIND]);
    let text = boids.to_text().expect("the sheet writes");
    let placed = report(&text);
    let error = placed.errors().next().expect("one error");
    let span = error.diagnostic.span.clone().expect("placed");
    assert!(text[span.start..].starts_with("[[agents]]\nid = \"hawk\""));
}

#[test]
fn a_grid_sheet_with_no_property_gets_the_empty_state_warning() {
    let text = "format = \"henad-model\"\nformat_version = 1\nkind = \"sheet\"\n\n[sheet]\nversion = 1\nid = \
                \"blank\"\nname = \"Untitled model\"\nnext_line = 1\n\n[world]\nmade_of = \"cells\"\nwidth = 100\nheight \
                = 100\nedges = \"wrap\"\n";
    let empty = report(text);
    let messages: Vec<(&str, DiagnosticSeverity)> = empty
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.diagnostic.message.as_str(), diagnostic.diagnostic.severity))
        .collect();
    assert_eq!(messages, [(SHEET_EMPTY, DiagnosticSeverity::Warning)]);
    // A blank model's card holds no line and is no problem.
    let with_card =
        format!("{text}\n[[card]]\nid = \"cell_tick\"\nevery = \"cells\"\nruns = \"each_tick\"\nlines = []\n");
    assert_eq!(report(&with_card).diagnostics.len(), 1);
    assert!(
        starters()
            .iter()
            .all(|starter| ParsedSheet::parse(starter.text).is_ok_and(|sheet| sheet.check().diagnostics.is_empty()))
    );
}

/// Returns the relation notes of card `card` of `text`, every line running.
fn relation_notes(text: &str, card: usize) -> Vec<RelationNote> {
    let document = read(text);
    let relations = CardRelations::new(&document, &document.cards[card]);
    let report = check::check(&document);
    relations.notes(&Sentences::new(&document), |line| !report.is_skipped(line))
}

/// Returns the words of the notes of `kind` among `notes`, each with the line it sits under.
fn notes_of(notes: &[RelationNote], keep: impl Fn(&RelationNoteKind) -> bool) -> Vec<(String, String)> {
    notes
        .iter()
        .filter(|note| keep(&note.kind))
        .map(|note| (note.line.clone(), note.text.clone()))
        .collect()
}

#[test]
fn the_order_note_shows_on_game_of_life_and_sir_and_not_on_the_vote_or_the_forest_fire() {
    let order = |file_name: &str| {
        notes_of(&relation_notes(sheet_text(file_name), 0), |kind| {
            *kind == RelationNoteKind::Order
        })
    };
    assert_eq!(
        order("game_of_life.toml"),
        [(
            "l3".to_owned(),
            "Lines 2 and 3 change different cells (Dead and Alive), so their order does not matter.".to_owned()
        )]
    );
    assert_eq!(
        order("sir.toml"),
        [(
            "l3".to_owned(),
            "Lines 2 and 3 change different cells (Susceptible and Infected), so their order does not matter."
                .to_owned()
        )]
    );
    assert!(order("vote.toml").is_empty());
    assert!(order("forest_fire.toml").is_empty());
    // A line that does not run takes the note away.
    let skipped = edited_sheet(
        "game_of_life.toml",
        &[(
            "[card.line.l2.condition]\nkind = \"compare\"\ntest = \"is\"\nleft = { result = \"l1\" }\nright = 3\n",
            "",
        )],
    );
    let notes = relation_notes(&skipped, 0);
    assert!(
        notes_of(&notes, |kind| *kind == RelationNoteKind::Order).is_empty(),
        "{notes:?}"
    );
}

#[test]
fn the_vote_is_covered_and_a_gap_between_two_thresholds_is_named() {
    let notes = relation_notes(sheet_text("vote.toml"), 0);
    assert_eq!(
        notes_of(&notes, |kind| *kind == RelationNoteKind::Coverage),
        [(
            "l3".to_owned(),
            "Lines 2 and 3 cover every value of B in my block. A cell ends the tick A when it is 0 to 4 and B when it \
             is 5 to 9."
                .to_owned()
        )]
    );
    assert!(notes_of(&notes, |kind| *kind == RelationNoteKind::Gap).is_empty());
    // "Become A from any state, when B in my block is at most 3" leaves 4 to no line.
    let gap = edited_sheet(
        "vote.toml",
        &[(
            "[card.line.l3.condition]\nkind = \"always\"\n",
            "[card.line.l3.condition]\nkind = \"compare\"\ntest = \"at_most\"\nleft = { result = \"l1\" }\nright = 3\n",
        )],
    );
    let notes = relation_notes(&gap, 0);
    assert_eq!(
        notes_of(&notes, |kind| *kind == RelationNoteKind::Gap),
        [(
            "l3".to_owned(),
            "None of lines 2 and 3 applies when B in my block is 4. Those cells keep their Opinion.".to_owned()
        )]
    );
    // With a gap, a cell's end state depends on its own state, so no coverage note shows.
    assert!(notes_of(&notes, |kind| *kind == RelationNoteKind::Coverage).is_empty());
    // Lines that read the count from below this line do not run, and the gap note leaves them out.
    let skipped = gap.replacen(
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l2\", \"l3\", \"l1\"]",
        1,
    );
    let report = check::check(&read(&skipped));
    assert!(report.is_skipped("l2") && report.is_skipped("l3"));
    let notes = relation_notes(&skipped, 0);
    assert!(
        notes_of(&notes, |kind| *kind == RelationNoteKind::Gap).is_empty(),
        "{notes:?}"
    );
    // Game of Life's lines change from different states, so the end state depends on the cell's own state.
    let life = relation_notes(sheet_text("game_of_life.toml"), 0);
    assert!(
        notes_of(&life, |kind| matches!(
            kind,
            RelationNoteKind::Coverage | RelationNoteKind::Gap
        ))
        .is_empty()
    );
}

/// Returns Game of Life with its card's lines `lines` and the When line `l5` testing `condition`, holding `inside`,
/// followed by the tables `more`.
fn life_with_when(lines: &str, condition: &str, inside: &str, more: &str) -> String {
    let when = format!(
        "[card.line.l5]\nkind = \"when\"\nlines = {inside}\n\n[card.line.l5.condition]\n{condition}\n\n\
         {more}# Lines of Clear"
    );
    edited_sheet(
        "game_of_life.toml",
        &[
            ("next_line = 5", "next_line = 7"),
            ("lines = [\"l1\", \"l2\", \"l3\"]", lines),
            ("# Lines of Clear", &when),
        ],
    )
}

#[test]
fn a_when_offers_to_fold_into_its_lines_and_notes_what_holds_inside_it() {
    let dead = "kind = \"compare\"\ntest = \"is\"\nleft = { my = \"status\" }\nright = { state = \"dead\" }";
    // A When of "my Status is Dead" around the Become from Dead repeats it.
    let repeats = life_with_when("lines = [\"l1\", \"l5\", \"l3\"]", dead, "[\"l2\"]", "");
    let notes = relation_notes(&repeats, 0);
    assert_eq!(
        notes_of(&notes, |kind| *kind == RelationNoteKind::WhenRepeats),
        [(
            "l5".to_owned(),
            "Line 3 already says from Dead, so this When repeats it.".to_owned()
        )]
    );
    assert_eq!(
        notes_of(&notes, |kind| *kind == RelationNoteKind::Inside),
        [("l2".to_owned(), "Only when line 2 holds: my Status is Dead.".to_owned())]
    );
    // A When around one change that holds always offers to move its condition into the change.
    let three = "kind = \"compare\"\ntest = \"is\"\nleft = { result = \"l1\" }\nright = 3";
    let mut moved = life_with_when("lines = [\"l1\", \"l5\", \"l3\"]", three, "[\"l2\"]", "");
    moved = moved.replacen(
        "[card.line.l2.condition]\nkind = \"compare\"\ntest = \"is\"\nleft = { result = \"l1\" }\nright = 3\n",
        "[card.line.l2.condition]\nkind = \"always\"\n",
        1,
    );
    let notes = relation_notes(&moved, 0);
    let fold: Vec<&RelationNote> = notes
        .iter()
        .filter(|note| matches!(note.kind, RelationNoteKind::OnlyOneInside { .. }))
        .collect();
    assert_eq!(fold.len(), 1, "{notes:?}");
    assert_eq!(fold[0].line, "l5");
    assert_eq!(
        fold[0].text,
        "Only line 3 is inside. Move this condition into that line's own when."
    );
    assert_eq!(
        fold[0].kind,
        RelationNoteKind::OnlyOneInside {
            inside: "l2".to_owned(),
            number: 3
        }
    );
    // A result inside a When is worked out for every cell, and an empty When holds nothing.
    let result_inside = life_with_when("lines = [\"l5\", \"l2\", \"l3\"]", three, "[\"l1\"]", "");
    let notes = relation_notes(&result_inside, 0);
    assert_eq!(
        notes_of(&notes, |kind| *kind == RelationNoteKind::Inside),
        [(
            "l1".to_owned(),
            "Worked out for every cell, whether or not line 1 holds. Only lines inside line 1 can use it.".to_owned()
        )]
    );
    let empty = life_with_when("lines = [\"l1\", \"l2\", \"l3\", \"l5\"]", three, "[]", "");
    let notes = relation_notes(&empty, 0);
    assert_eq!(
        notes_of(&notes, |kind| *kind == RelationNoteKind::NothingInside),
        [("l5".to_owned(), "Nothing inside yet.".to_owned())]
    );
    // A When holding another When offers no fold, and the inner When notes its outer one.
    let inner = format!("[card.line.l6]\nkind = \"when\"\nlines = [\"l2\"]\n\n[card.line.l6.condition]\n{three}\n\n");
    let nested = life_with_when("lines = [\"l1\", \"l5\", \"l3\"]", dead, "[\"l6\"]", &inner);
    let notes = relation_notes(&nested, 0);
    assert!(
        notes
            .iter()
            .all(|note| note.line != "l5" || note.kind == RelationNoteKind::Inside),
        "{notes:?}"
    );
    assert!(
        notes_of(&notes, |kind| *kind == RelationNoteKind::Inside)
            .contains(&("l6".to_owned(), "Only when line 2 holds: my Status is Dead.".to_owned()))
    );
}

/// A by-chance start with its chance left empty is an error that names the property, in the sheet's words.
#[test]
fn an_empty_start_chance_names_its_property() {
    let mut sir = read(sheet_text("sir.toml"));
    let cells = sir.cells.as_mut().expect("cells");
    let start = cells[0].starts_as.as_mut().expect("a start");
    start.rows[0].chance = Slot::Blank;
    let errors: Vec<String> = check::check(&sir)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.diagnostic.severity == DiagnosticSeverity::Error)
        .map(|diagnostic| diagnostic.diagnostic.message)
        .collect();
    assert_eq!(errors, [sentence::empty_field("Health", "a chance")]);
}
