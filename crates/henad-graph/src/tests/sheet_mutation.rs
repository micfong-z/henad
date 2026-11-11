//! Mutation tests of the sheet checker: one broken copy of a committed sheet per rule of the author format, each
//! reported at the right table as an error or as a warning, and the checker run over the reader's mutation corpus.

use std::panic::{AssertUnwindSafe, catch_unwind};

use henad_core::diagnostic::DiagnosticSeverity;
use henad_core::explore::fingerprint::fnv1a64;

use super::sheet_document::{edited, mutate, sheets};
use crate::sheet::check::SheetDiagnostic;
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{
    ARROW_COMPARED_WITH_NUMBER, CHANCE_LIMITS_OUTSIDE, CHANCE_OUT_OF_RANGE, NOT_RUN_TICK, RATE_BELOW_ZERO,
    RATE_IN_BUTTON_CARD,
};
use crate::sheet::{ParsedSheet, check};

/// Kind of a diagnostic a mutation must produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// An error, which blocks a build.
    Error,
    /// A warning that keeps the line running.
    Warning,
    /// A skip warning, which keeps its line from running.
    Skip,
}

/// Checks that checking `text` reports `message` as `expected`, placed at the table whose text starts with `at`.
fn assert_reported(text: &str, expected: Expected, message: &str, at: &str) -> SheetDiagnostic {
    let sheet = ParsedSheet::parse(text).unwrap_or_else(|error| panic!("the mutation does not read: {error}"));
    let report = sheet.check();
    let found = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.diagnostic.message == message
                && diagnostic
                    .diagnostic
                    .span
                    .as_ref()
                    .is_some_and(|span| text[span.start..].starts_with(at))
        })
        .unwrap_or_else(|| {
            let all: Vec<String> = report.diagnostics.iter().map(|d| d.diagnostic.to_string()).collect();
            panic!("no diagnostic {message:?} at {at:?} among:\n{}", all.join("\n"))
        });
    let (severity, skips) = match expected {
        Expected::Error => (DiagnosticSeverity::Error, false),
        Expected::Warning => (DiagnosticSeverity::Warning, false),
        Expected::Skip => (DiagnosticSeverity::Warning, true),
    };
    assert_eq!(found.diagnostic.severity, severity, "{}", found.diagnostic);
    assert_eq!(found.skips_line, skips, "{}", found.diagnostic);
    let span = found
        .diagnostic
        .span
        .clone()
        .unwrap_or_else(|| panic!("{} has no place", found.diagnostic));
    assert!(
        text[span.start..].starts_with(at),
        "{} is placed at {:?}, not {at:?}",
        found.diagnostic,
        &text[span.clone()]
    );
    if let Some(line) = &found.line {
        assert!(at.starts_with(&format!("[card.line.{line}]")), "{line} against {at:?}");
        assert_eq!(report.is_skipped(line), skips || report.skip_note(line).is_some());
    }
    found.clone()
}

#[test]
fn a_result_below_its_user_skips_the_line() {
    let text = edited(
        "game_of_life.toml",
        &[("lines = [\"l1\", \"l2\", \"l3\"]", "lines = [\"l2\", \"l1\", \"l3\"]")],
    );
    assert_reported(
        &text,
        Expected::Skip,
        "Uses Alive neighbours, which is below this line",
        "[card.line.l2]",
    );
}

#[test]
fn a_result_of_a_change_line_is_an_error() {
    let text = edited(
        "game_of_life.toml",
        &[(
            "left = { result = \"l1\" }\nright = 3",
            "left = { result = \"l3\" }\nright = 3",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "l3 makes a change and gives no result",
        "[card.line.l2]",
    );
}

#[test]
fn an_unknown_parameter_is_an_error() {
    let text = edited(
        "forest_fire.toml",
        &[(
            "chance = { param = \"lightning_chance\" }",
            "chance = { param = \"lightning\" }",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "the parameter 'lightning' does not exist",
        "[card.line.l3]",
    );
}

#[test]
fn a_state_of_another_property_is_an_error() {
    let text = edited(
        "ants.toml",
        &[(
            "[card.line.l3.condition]\nkind = \"compare\"\ntest = \"is\"\nleft = { my = \"cargo\" }\nright = { state = \
             \"searching\" }",
            "[card.line.l3.condition]\nkind = \"compare\"\ntest = \"is\"\nleft = { my = \"cargo\" }\nright = { state = \
             \"food\" }",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "'food' is no state of 'cargo'",
        "[card.line.l3]",
    );
}

#[test]
fn my_new_above_the_last_change_skips_the_line() {
    let text = edited(
        "boids.toml",
        &[(
            "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\"]",
            "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l7\", \"l6\"]",
        )],
    );
    assert_reported(
        &text,
        Expected::Skip,
        "Uses my new Velocity, which is below this line",
        "[card.line.l7]",
    );
}

#[test]
fn a_missing_key_is_a_warning() {
    let text = edited("forest_fire.toml", &[("key = 3688405984\n", "")]);
    let found = assert_reported(
        &text,
        Expected::Warning,
        "line l3 draws and holds no key",
        "[card.line.l3]",
    );
    assert!(!found.skips_line);
}

#[test]
fn a_duplicate_key_is_an_error() {
    let text = edited("forest_fire.toml", &[("key = 1270439170", "key = 3688405984")]);
    assert_reported(
        &text,
        Expected::Error,
        "line l5 holds key 3688405984, which line l3 also holds",
        "[card.line.l5]",
    );
}

#[test]
fn an_unlisted_line_is_an_error() {
    let text = edited(
        "game_of_life.toml",
        &[("lines = [\"l1\", \"l2\", \"l3\"]", "lines = [\"l1\", \"l2\"]")],
    );
    assert_reported(
        &text,
        Expected::Error,
        "line l3 is listed in no lines list",
        "[card.line.l3]",
    );
}

#[test]
fn a_line_past_next_line_is_an_error() {
    let text = edited("game_of_life.toml", &[("next_line = 5", "next_line = 4")]);
    assert_reported(
        &text,
        Expected::Error,
        "line l4 is not below [sheet] next_line 4",
        "[card.line.l4]",
    );
}

#[test]
fn a_rate_with_no_time_skips_the_line() {
    let text = edited("sir.toml", &[("[time]\ntick_length = 1\ntick_unit = \"day\"\n\n", "")]);
    assert_reported(&text, Expected::Skip, NOT_RUN_TICK, "[card.line.l2]");
    assert_reported(&text, Expected::Skip, NOT_RUN_TICK, "[card.line.l3]");
}

#[test]
fn a_number_in_a_condition_is_an_error() {
    let text = edited(
        "game_of_life.toml",
        &[(
            "start_state = \"any_state\"\n\n[card.line.l4.condition]\nkind = \"always\"",
            "start_state = \"any_state\"\ncondition = 3",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "a slot wants yes or no and holds a number",
        "[card.line.l4]",
    );
}

#[test]
fn a_count_in_a_button_card_is_an_error() {
    let text = edited(
        "game_of_life.toml",
        &[(
            "id = \"cell_tick\"\nevery = \"cells\"\nruns = \"each_tick\"",
            "id = \"cell_tick\"\nevery = \"cells\"\nruns = \"once\"",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "line l1: a card that runs once cannot look around",
        "[card.line.l1]",
    );
}

#[test]
fn a_result_from_another_card_is_an_error() {
    let text = edited(
        "game_of_life.toml",
        &[(
            "[card.line.l4.condition]\nkind = \"always\"",
            "[card.line.l4.condition]\nkind = \"compare\"\ntest = \"is\"\nleft = { result = \"l1\" }\nright = 3",
        )],
    );
    assert_reported(&text, Expected::Error, "l1 sits in another card", "[card.line.l4]");
}

#[test]
fn their_outside_a_look_is_an_error() {
    let text = edited(
        "game_of_life.toml",
        &[(
            "left = { result = \"l1\" }\nright = 3",
            "left = { their = \"status\" }\nright = 3",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "'their' is read outside a look around",
        "[card.line.l2]",
    );
}

#[test]
fn an_unknown_formula_word_is_an_error() {
    let text = edited(
        "ants.toml",
        &[(
            "formula = \"trail_falloff * l2.amount + my.reward_held\"",
            "formula = \"trail_fallof * l2.amount + my.reward_held\"",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "the parameter 'trail_fallof' does not exist",
        "[card.line.l3]",
    );
}

#[test]
fn a_formula_syntax_error_is_an_error() {
    let text = edited(
        "ants.toml",
        &[(
            "formula = \"trail_falloff * l2.amount + my.reward_held\"",
            "formula = \"trail_falloff * * l2.amount\"",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "the formula holds an unexpected '*'",
        "[card.line.l3]",
    );
}

#[test]
fn a_part_of_the_wrong_line_is_an_error() {
    let text = edited(
        "boids.toml",
        &[(
            "value = { result = \"l2\", part = \"push\" }",
            "value = { result = \"l2\", part = \"centre\" }",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "'centre' is no part of l2, a look_around line",
        "[card.line.l3]",
    );
}

#[test]
fn leave_on_an_agent_property_is_an_error() {
    let text = edited(
        "ants.toml",
        &[("ground_property = \"to_home\"", "ground_property = \"reward_held\"")],
    );
    assert_reported(
        &text,
        Expected::Error,
        "Leave needs a Number property of the ground, and 'reward_held' is not one",
        "[card.line.l3]",
    );
}

#[test]
fn become_on_a_number_is_an_error() {
    let text = edited(
        "ants.toml",
        &[(
            "kind = \"become\"\nproperty = \"cargo\"\nnew_state = \"carrying_food\"",
            "kind = \"become\"\nproperty = \"reward_held\"\nnew_state = \"carrying_food\"",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "Become changes a States property, and 'reward_held' is not one",
        "[card.line.l10]",
    );
}

#[test]
fn two_parameters_with_one_id_are_an_error() {
    let text = edited("sir.toml", &[("id = \"recovery_rate\"", "id = \"infection_rate\"")]);
    assert_reported(
        &text,
        Expected::Error,
        "the parameter id 'infection_rate' is also the id of a parameter",
        "[[parameter]]\nid = \"infection_rate\"\nname = \"Recovery rate\"",
    );
}

#[test]
fn a_parameter_id_of_the_line_form_is_an_error() {
    let text = edited("sir.toml", &[("id = \"recovery_rate\"", "id = \"l7\"")]);
    assert_reported(
        &text,
        Expected::Error,
        "the parameter id 'l7' is reserved",
        "[[parameter]]\nid = \"l7\"",
    );
}

#[test]
fn a_default_outside_its_range_is_an_error() {
    let text = edited("game_of_life.toml", &[("default = 0.3", "default = 1.5")]);
    assert_reported(
        &text,
        Expected::Error,
        "the default 1.5 of parameter 'initial_density' is outside its range, 0 to 1",
        "[[parameter]]\nid = \"initial_density\"",
    );
}

#[test]
fn an_unknown_state_in_a_measure_is_an_error() {
    let text = edited(
        "game_of_life.toml",
        &[(
            "property = \"status\"\nstate = \"alive\"",
            "property = \"status\"\nstate = \"alight\"",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "measure 'alive' counts the state 'alight', which is no state of 'status'",
        "[[measure]]\nid = \"alive\"",
    );
}

#[test]
fn a_button_running_a_tick_card_is_an_error() {
    let text = edited("game_of_life.toml", &[("card = \"clear\"", "card = \"cell_tick\"")]);
    assert_reported(
        &text,
        Expected::Error,
        "button 'clear' runs 'cell_tick', which is no card that runs once",
        "[[button]]\nid = \"clear\"",
    );
}

#[test]
fn a_count_inside_a_repeat_is_an_error() {
    let text = edited(
        "random_walk.toml",
        &[("[card.line.l2]\nkind = \"move\"", "[card.line.l2]\nkind = \"count\"")],
    );
    assert_reported(
        &text,
        Expected::Error,
        "A look around, a count or a pick of a cell cannot sit inside a Repeat.",
        "[card.line.l2]",
    );
}

#[test]
fn a_rate_in_a_run_once_card_is_an_error() {
    let text = edited(
        "sir.toml",
        &[(
            "kind = \"chance\"\nchance = { param = \"initial_infected\" }",
            "kind = \"rate\"\nrate = { param = \"recovery_rate\" }\nrate_unit = \"day\"",
        )],
    );
    assert_reported(&text, Expected::Error, RATE_IN_BUTTON_CARD, "[card.line.l4]");
}

#[test]
fn a_chance_above_1_and_a_negative_rate_are_errors() {
    for chance in ["1.5", "-0.1", "{ percent = 150 }"] {
        let text = edited(
            "forest_fire.toml",
            &[(
                "chance = { param = \"lightning_chance\" }",
                &format!("chance = {chance}"),
            )],
        );
        assert_reported(&text, Expected::Error, CHANCE_OUT_OF_RANGE, "[card.line.l3]");
    }
    for chance in ["1", "0", "{ percent = 30 }", "{ percent = 100 }"] {
        let text = edited(
            "forest_fire.toml",
            &[(
                "chance = { param = \"lightning_chance\" }",
                &format!("chance = {chance}"),
            )],
        );
        let report = ParsedSheet::parse(&text).expect("the sheet reads").check();
        assert!(report.diagnostics.is_empty(), "{chance}: {:?}", report.diagnostics);
    }
    for rate in ["-0.1", "{ percent = -5 }"] {
        let text = edited(
            "sir.toml",
            &[("rate = { param = \"recovery_rate\" }", &format!("rate = {rate}"))],
        );
        assert_reported(&text, Expected::Error, RATE_BELOW_ZERO, "[card.line.l3]");
    }
    // A chance outside 0 to 1 is refused where a cell starts, too.
    let text = edited(
        "game_of_life.toml",
        &[("chance = { param = \"initial_density\" }", "chance = { percent = 130 }")],
    );
    assert_reported(&text, Expected::Error, CHANCE_OUT_OF_RANGE, "[[cells.property]]");
}

#[test]
fn a_summary_id_follows_the_id_rules() {
    let long = "p".repeat(49);
    for (id, message) in [
        (
            long.as_str(),
            format!("the summary id '{long}' holds 49 bytes, and a sheet id holds at most 48"),
        ),
        (
            "Push!",
            "the summary id 'Push!' does not follow the id grammar".to_owned(),
        ),
        ("count", "the summary id 'count' is reserved".to_owned()),
        ("l4", "the summary id 'l4' is reserved".to_owned()),
    ] {
        let text = edited(
            "boids.toml",
            &[
                ("{ id = \"push\",", &format!("{{ id = \"{id}\",")),
                ("part = \"push\"", &format!("part = \"{id}\"")),
            ],
        );
        assert_reported(&text, Expected::Error, &message, "[card.line.l2]");
    }
}

#[test]
fn a_chance_parameter_ranging_past_1_is_an_error() {
    let text = edited("forest_fire.toml", &[("max = 0.001", "max = 2")]);
    assert_reported(&text, Expected::Error, CHANCE_LIMITS_OUTSIDE, "[card.line.l3]");
    let text = edited(
        "sir.toml",
        &[(
            "id = \"recovery_rate\"\nname = \"Recovery rate\"\ntype = \"number\"\nmin = 0",
            "id = \"recovery_rate\"\nname = \"Recovery rate\"\ntype = \"number\"\nmin = -1",
        )],
    );
    assert_reported(&text, Expected::Error, RATE_BELOW_ZERO, "[card.line.l3]");
}

/// Reads `text`, checks it and walks the report and the relations of every card. A panic fails the test with the text.
fn exercise(text: &str) -> bool {
    let outcome = catch_unwind(AssertUnwindSafe(|| match ParsedSheet::parse(text) {
        Ok(sheet) => {
            let report = sheet.check();
            assert_eq!(
                check::check(sheet.document()).diagnostics.len(),
                report.diagnostics.len()
            );
            for diagnostic in &report.diagnostics {
                assert!(!diagnostic.diagnostic.message.is_empty());
                assert!(
                    diagnostic.diagnostic.line.is_some(),
                    "{} has no place",
                    diagnostic.diagnostic
                );
            }
            for card in &sheet.document().cards {
                let relations = CardRelations::new(sheet.document(), card);
                for id in relations.order() {
                    relations.otherwise_numbers(id);
                }
                relations.competing_groups();
            }
            assert!(sheet.document().to_text().is_ok());
            true
        }
        Err(_) => false,
    }));
    outcome.unwrap_or_else(|_| panic!("the sheet checker panicked on:\n{text}"))
}

#[test]
fn the_sheet_checker_never_panics_on_the_mutation_corpus() {
    let (mut cases, mut read) = (0usize, 0usize);
    for (name, text) in sheets() {
        // Each sheet draws its own edits, so a sheet added to the corpus leaves the others' edits as they were.
        let mut rng = 0x6368_6563_6b00_0005u64 ^ fnv1a64(name.as_bytes());
        for edit in 0..400 {
            let mut edited = mutate(text, &mut rng);
            if edit % 3 == 0 {
                edited = mutate(&edited, &mut rng);
            }
            cases += 1;
            read += usize::from(exercise(&edited));
        }
        // Every key of every table deleted in turn, which leaves most slots blank once.
        for (index, line) in text.lines().enumerate() {
            if line.contains(" = ") {
                let edited: Vec<&str> = text
                    .lines()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .map(|(_, line)| line)
                    .collect();
                cases += 1;
                read += usize::from(exercise(&edited.join("\n")));
            }
        }
    }
    assert!(
        read * 4 > cases,
        "only {read} of {cases} edits read, so the corpus tests the TOML parser alone"
    );
}

#[test]
fn a_comparison_of_an_arrow_with_a_number_is_an_error_at_its_line() {
    let compare = |test: &str, left: &str, right: &str| {
        edited(
            "boids.toml",
            &[(
                "[card.line.l8.condition]\nkind = \"always\"",
                &format!("[card.line.l8.condition]\nkind = \"compare\"\ntest = \"{test}\"\nleft = {left}\n{right}"),
            )],
        )
    };
    let velocity = "{ my = \"velocity\" }";
    for text in [
        compare("more_than", velocity, "right = 1"),
        compare("at_most", "2", "right = { my = \"velocity\" }"),
        compare("outside", velocity, "low = 1\nhigh = 2"),
    ] {
        assert_reported(&text, Expected::Error, ARROW_COMPARED_WITH_NUMBER, "[card.line.l8]");
    }
    // A number compared with the length of the arrow checks clean.
    let length = compare(
        "more_than",
        "{ kind = \"calculate\", formula = \"length(my.velocity)\" }",
        "right = 1",
    );
    let report = ParsedSheet::parse(&length).expect("the sheet reads").check();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
}
