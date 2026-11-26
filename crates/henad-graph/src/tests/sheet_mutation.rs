//! Mutation tests of the sheet checker: one broken copy of a committed sheet per rule of the author format, each
//! reported at the right table as an error or as a warning, and the checker run over the reader's mutation corpus.

use std::panic::{AssertUnwindSafe, catch_unwind};

use henad_core::diagnostic::DiagnosticSeverity;
use henad_core::explore::fingerprint::fnv1a64;

use super::sheet_support::{SHEET_HOSTILE, VALUES, edited_sheet, sheets};
use super::support::{mutations, without_each_key};
use crate::check::RUNS_CSV_COLUMNS;
use crate::sheet::check::SheetDiagnostic;
use crate::sheet::limits::MAX_FORMULA_DEPTH;
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{
    ARROW_COMPARED_WITH_NUMBER, CHANCE_LIMITS_OUTSIDE, CHANCE_OUT_OF_RANGE, NOT_RUN_TICK, RATE_BELOW_ZERO,
    RATE_IN_BUTTON_CARD,
};
use crate::sheet::{ParsedSheet, SheetFile, check, sentence};

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
            let all: Vec<String> = report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.diagnostic.to_string())
                .collect();
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
    let text = edited_sheet(
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
    let text = edited_sheet(
        "game_of_life.toml",
        &[(
            "left = { result = \"l1\" }\nright = 3",
            "left = { result = \"l3\" }\nright = 3",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "l3 makes a change and returns no result",
        "[card.line.l2]",
    );
}

#[test]
fn an_unknown_parameter_is_an_error() {
    let text = edited_sheet(
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
    let text = edited_sheet(
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
        "'food' is not a state of 'cargo'",
        "[card.line.l3]",
    );
}

#[test]
fn my_new_above_the_last_change_skips_the_line() {
    let text = edited_sheet(
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
    let text = edited_sheet("forest_fire.toml", &[("key = 3688405984\n", "")]);
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
    let text = edited_sheet("forest_fire.toml", &[("key = 1270439170", "key = 3688405984")]);
    assert_reported(
        &text,
        Expected::Error,
        "line l5 holds key 3688405984, which line l3 also holds",
        "[card.line.l5]",
    );
}

#[test]
fn an_unlisted_line_is_an_error() {
    let text = edited_sheet(
        "game_of_life.toml",
        &[("lines = [\"l1\", \"l2\", \"l3\"]", "lines = [\"l1\", \"l2\"]")],
    );
    assert_reported(
        &text,
        Expected::Error,
        "line l3 is not listed in any lines list",
        "[card.line.l3]",
    );
}

#[test]
fn a_line_past_next_line_is_an_error() {
    let text = edited_sheet("game_of_life.toml", &[("next_line = 5", "next_line = 4")]);
    assert_reported(
        &text,
        Expected::Error,
        "the number of line l4 must be less than [sheet] next_line 4",
        "[card.line.l4]",
    );
}

#[test]
fn a_rate_with_no_time_skips_the_line() {
    let text = edited_sheet("sir.toml", &[("[time]\ntick_length = 1\ntick_unit = \"day\"\n\n", "")]);
    assert_reported(&text, Expected::Skip, NOT_RUN_TICK, "[card.line.l2]");
    assert_reported(&text, Expected::Skip, NOT_RUN_TICK, "[card.line.l3]");
}

#[test]
fn a_number_in_a_condition_is_an_error() {
    let text = edited_sheet(
        "game_of_life.toml",
        &[(
            "start_state = \"any_state\"\n\n[card.line.l4.condition]\nkind = \"always\"",
            "start_state = \"any_state\"\ncondition = 3",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "expected a Boolean in a slot, got a number",
        "[card.line.l4]",
    );
}

#[test]
fn a_count_in_a_button_card_is_an_error() {
    let text = edited_sheet(
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
    let text = edited_sheet(
        "game_of_life.toml",
        &[(
            "[card.line.l4.condition]\nkind = \"always\"",
            "[card.line.l4.condition]\nkind = \"compare\"\ntest = \"is\"\nleft = { result = \"l1\" }\nright = 3",
        )],
    );
    assert_reported(&text, Expected::Error, "l1 is in another card", "[card.line.l4]");
}

#[test]
fn their_outside_a_look_is_an_error() {
    let text = edited_sheet(
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
    let text = edited_sheet(
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
    let text = edited_sheet(
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
    let text = edited_sheet(
        "boids.toml",
        &[(
            "value = { result = \"l2\", part = \"push\" }",
            "value = { result = \"l2\", part = \"centre\" }",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "'centre' is not a part of l2, a look_around line",
        "[card.line.l3]",
    );
}

#[test]
fn leave_on_an_agent_property_is_an_error() {
    let text = edited_sheet(
        "ants.toml",
        &[("ground_property = \"to_home\"", "ground_property = \"reward_held\"")],
    );
    assert_reported(
        &text,
        Expected::Error,
        "Leave needs a Number property of the ground, got 'reward_held'",
        "[card.line.l3]",
    );
}

#[test]
fn become_on_a_number_is_an_error() {
    let text = edited_sheet(
        "ants.toml",
        &[(
            "kind = \"become\"\nproperty = \"cargo\"\nnew_state = \"carrying_food\"",
            "kind = \"become\"\nproperty = \"reward_held\"\nnew_state = \"carrying_food\"",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "Become needs a States property, got 'reward_held'",
        "[card.line.l10]",
    );
}

#[test]
fn two_parameters_with_one_id_are_an_error() {
    let text = edited_sheet("sir.toml", &[("id = \"recovery_rate\"", "id = \"infection_rate\"")]);
    assert_reported(
        &text,
        Expected::Error,
        "the parameter id 'infection_rate' is also the id of a parameter",
        "[[parameter]]\nid = \"infection_rate\"\nname = \"Recovery rate\"",
    );
}

#[test]
fn a_parameter_id_of_the_line_form_is_an_error() {
    let text = edited_sheet("sir.toml", &[("id = \"recovery_rate\"", "id = \"l7\"")]);
    assert_reported(
        &text,
        Expected::Error,
        "the parameter id 'l7' is reserved",
        "[[parameter]]\nid = \"l7\"",
    );
}

#[test]
fn a_default_outside_its_range_is_an_error() {
    let text = edited_sheet("game_of_life.toml", &[("default = 0.3", "default = 1.5")]);
    assert_reported(
        &text,
        Expected::Error,
        "the default 1.5 of parameter 'initial_density' is outside its range, 0 to 1",
        "[[parameter]]\nid = \"initial_density\"",
    );
}

#[test]
fn an_unknown_state_in_a_measure_is_an_error() {
    let text = edited_sheet(
        "game_of_life.toml",
        &[(
            "property = \"status\"\nstate = \"alive\"",
            "property = \"status\"\nstate = \"alight\"",
        )],
    );
    assert_reported(
        &text,
        Expected::Error,
        "measure 'alive' counts the state 'alight', which is not a state of 'status'",
        "[[measure]]\nid = \"alive\"",
    );
}

#[test]
fn a_button_running_a_tick_card_is_an_error() {
    let text = edited_sheet("game_of_life.toml", &[("card = \"clear\"", "card = \"cell_tick\"")]);
    assert_reported(
        &text,
        Expected::Error,
        "button 'clear' runs 'cell_tick', which is not a card that runs once",
        "[[button]]\nid = \"clear\"",
    );
}

#[test]
fn a_count_inside_a_repeat_is_an_error() {
    let text = edited_sheet(
        "random_walk.toml",
        &[("[card.line.l2]\nkind = \"move\"", "[card.line.l2]\nkind = \"count\"")],
    );
    assert_reported(
        &text,
        Expected::Error,
        "A look around, a count or a pick of a cell cannot be used inside a Repeat.",
        "[card.line.l2]",
    );
}

#[test]
fn a_rate_in_a_run_once_card_is_an_error() {
    let text = edited_sheet(
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
        let text = edited_sheet(
            "forest_fire.toml",
            &[(
                "chance = { param = \"lightning_chance\" }",
                &format!("chance = {chance}"),
            )],
        );
        assert_reported(&text, Expected::Error, CHANCE_OUT_OF_RANGE, "[card.line.l3]");
    }
    for chance in ["1", "0", "{ percent = 30 }", "{ percent = 100 }"] {
        let text = edited_sheet(
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
        let text = edited_sheet(
            "sir.toml",
            &[("rate = { param = \"recovery_rate\" }", &format!("rate = {rate}"))],
        );
        assert_reported(&text, Expected::Error, RATE_BELOW_ZERO, "[card.line.l3]");
    }
    // A chance outside 0 to 1 is refused where a cell starts, too.
    let text = edited_sheet(
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
            format!("the summary id '{long}' holds 49 bytes, expected at most 48"),
        ),
        ("Push!", "invalid summary id 'Push!'".to_owned()),
        ("count", "the summary id 'count' is reserved".to_owned()),
        ("l4", "the summary id 'l4' is reserved".to_owned()),
    ] {
        let text = edited_sheet(
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
    let text = edited_sheet("forest_fire.toml", &[("max = 0.001", "max = 2")]);
    assert_reported(&text, Expected::Error, CHANCE_LIMITS_OUTSIDE, "[card.line.l3]");
    let text = edited_sheet(
        "sir.toml",
        &[(
            "id = \"recovery_rate\"\nname = \"Recovery rate\"\ntype = \"number\"\nmin = 0",
            "id = \"recovery_rate\"\nname = \"Recovery rate\"\ntype = \"number\"\nmin = -1",
        )],
    );
    assert_reported(&text, Expected::Error, RATE_BELOW_ZERO, "[card.line.l3]");
}

/// A measure named `run_id`, `tick` or `time` is an error at its table, since exported stats or sweep results write
/// columns of those names. A name in another case is a name of its own.
#[test]
fn a_measure_named_run_id_tick_or_time_is_an_error() {
    let named = |name: &str| {
        edited_sheet(
            "sir.toml",
            &[(
                "id = \"infected\"\nname = \"Infected\"\ncolor",
                &format!("id = \"infected\"\nname = \"{name}\"\ncolor"),
            )],
        )
    };
    for name in ["run_id", "tick", "time"] {
        assert_reported(
            &named(name),
            Expected::Error,
            &sentence::measure_name_reserved(name),
            "[[measure]]\nid = \"infected\"",
        );
    }
    for name in ["Time", "Tick", "time spent"] {
        let report = ParsedSheet::parse(&named(name)).expect("the sheet reads").check();
        assert!(!report.has_errors(), "{name}: {:?}", report.diagnostics);
    }
}

/// A parameter whose id is a fixed column of `runs.csv`, such as `seed` or `status`, is an error at its table, since a
/// sweep writes the parameters among those columns. An id that holds more than a column's name is an id of its own.
#[test]
fn a_parameter_with_the_id_of_a_column_of_sweep_results_is_an_error() {
    let with_id = |id: &str| {
        edited_sheet(
            "sir.toml",
            &[
                ("id = \"recovery_rate\"", &format!("id = \"{id}\"")),
                ("param = \"recovery_rate\"", &format!("param = \"{id}\"")),
            ],
        )
    };
    for id in RUNS_CSV_COLUMNS {
        assert_reported(
            &with_id(id),
            Expected::Error,
            &sentence::parameter_id_reserved(id),
            &format!("[[parameter]]\nid = \"{id}\""),
        );
    }
    for id in ["seeds", "recovery_status", "notes"] {
        let report = ParsedSheet::parse(&with_id(id)).expect("the sheet reads").check();
        assert!(!report.has_errors(), "{id}: {:?}", report.diagnostics);
    }
}

/// An any or all summary of a value that is not yes or no is an error at the summary's own line, over cells and over
/// agents, and whether or not a line reads it.
#[test]
fn an_any_or_all_of_a_value_that_is_not_yes_or_no_is_an_error_at_its_line() {
    let values = VALUES.replace(
        "summary = \"any\", value = { their = \"flag\" }",
        "summary = \"any\", value = { their = \"level\" }",
    );
    assert_ne!(values, VALUES);
    assert_reported(
        &values,
        Expected::Error,
        "expected a Boolean in a slot, got a number",
        "[card.line.l2]",
    );
    let values = VALUES.replace(
        "summary = \"all\", value = { their = \"flag\" }",
        "summary = \"all\", value = { their = \"level\" }",
    );
    assert_reported(
        &values,
        Expected::Error,
        "expected a Boolean in a slot, got a number",
        "[card.line.l2]",
    );
    // A summary of the boids that no line reads.
    let boids = edited_sheet(
        "boids.toml",
        &[(
            "  { id = \"centre\",",
            "  { id = \"any_moving\", name = \"any moving\", summary = \"any\", value = { their = \"velocity\" } },\n  \
             { id = \"centre\",",
        )],
    );
    let error = assert_reported(
        &boids,
        Expected::Error,
        "expected a Boolean in a slot, got a vector",
        "[card.line.l1]",
    );
    assert_eq!(error.line.as_deref(), Some("l1"));
    let report = ParsedSheet::parse(&boids).expect("the sheet reads").check();
    assert_eq!(report.errors().count(), 1, "{:?}", report.diagnostics);
    // A yes or no summed up by any and all stays clean.
    let report = ParsedSheet::parse(VALUES).expect("the sheet reads").check();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
}

/// The trail kept each tick on the ants' To food, as the starter writes it.
const TO_FOOD_KEPT: &str = "id = \"to_food\"\nname = \"To food\"\ntype = \"number\"\nmarks = \"largest\"\nkept_per_tick = { param = \"trail_kept_per_tick\" }";

/// A share kept each tick outside 0 to 1 is a warning at its property, as a number and as the range of its parameter.
/// The lowering multiplies the marks by the share as it is, and the warning says so.
#[test]
fn a_share_kept_outside_0_to_1_warns() {
    let kept = |share: &str| {
        let edited = TO_FOOD_KEPT.replace("{ param = \"trail_kept_per_tick\" }", share);
        edited_sheet("ants.toml", &[(TO_FOOD_KEPT, &edited)])
    };
    for (share, words, grows) in [
        ("5", "5", true),
        ("-0.1", "-0.1", false),
        ("{ percent = 150 }", "150%", true),
    ] {
        let text = kept(share);
        assert_reported(
            &text,
            Expected::Warning,
            &sentence::kept_outside_note("To food", words, grows),
            "[[ground.property]]\nid = \"to_food\"",
        );
        let report = ParsedSheet::parse(&text).expect("the sheet reads").check();
        assert!(!report.has_errors(), "{share}: {:?}", report.diagnostics);
        SheetFile::parse(&text).unwrap_or_else(|error| panic!("{share} lowers: {error}"));
    }
    for share in ["1", "0", "{ percent = 99 }"] {
        let report = ParsedSheet::parse(&kept(share)).expect("the sheet reads").check();
        assert!(report.diagnostics.is_empty(), "{share}: {:?}", report.diagnostics);
    }
    // The parameter is the share of both trails, and each warns.
    for (edit, limit, grows) in [
        (("max = 1\ndefault = 0.999", "max = 2\ndefault = 0.999"), "2", true),
        (
            (
                "min = 0.9\nmax = 1\ndefault = 0.999",
                "min = -1\nmax = 1\ndefault = 0.999",
            ),
            "-1",
            false,
        ),
    ] {
        let text = edited_sheet("ants.toml", &[edit]);
        for (property, table) in [("To food", "id = \"to_food\""), ("To home", "id = \"to_home\"")] {
            assert_reported(
                &text,
                Expected::Warning,
                &sentence::kept_limits_outside_note("Trail kept per tick", property, limit, grows),
                &format!("[[ground.property]]\n{table}"),
            );
        }
        let report = ParsedSheet::parse(&text).expect("the sheet reads").check();
        assert!(!report.has_errors(), "{limit}: {:?}", report.diagnostics);
    }
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
        // Random edits, then every key of every table deleted in turn, which leaves most slots blank once.
        for edited in mutations(text, &mut rng, &SHEET_HOSTILE, 400).chain(without_each_key(text)) {
            cases += 1;
            read += usize::from(exercise(&edited));
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
        edited_sheet(
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

#[test]
fn a_world_or_a_population_past_the_model_limits_is_an_error_at_its_table() {
    let world = |file_name: &str, old: &str, new: &str| edited_sheet(file_name, &[(old, new)]);
    let cells = "width = 1024\nheight = 1024";
    let space = "width = 1000\nheight = 1000";
    assert_reported(
        &world("game_of_life.toml", cells, "width = 65536\nheight = 1024"),
        Expected::Error,
        "the world must be at most 16384 cells wide and 16384 cells high, got 65536 by 1024",
        "[world]",
    );
    assert_reported(
        &world("boids.toml", space, "width = 1000\nheight = 4294967295"),
        Expected::Error,
        "the world must be at most 10000 cells wide and 10000 cells high, got 1000 by 4294967295",
        "[world]",
    );
    assert_reported(
        &world("boids.toml", "max_count = 1000000", "max_count = 20000000"),
        Expected::Error,
        "the max_count of the agents 'bird' must be at most 10000000, got 20000000",
        "[[agents]]",
    );
    // A world and a population at the limits check clean.
    for text in [
        world("game_of_life.toml", cells, "width = 16384\nheight = 16384"),
        world("boids.toml", space, "width = 10000\nheight = 10000"),
        world("boids.toml", "max_count = 1000000", "max_count = 10000000"),
    ] {
        let report = ParsedSheet::parse(&text).expect("the sheet reads").check();
        assert!(!report.has_errors(), "{:?}", report.diagnostics);
    }
}

#[test]
fn a_depending_on_value_for_no_state_is_an_error_at_its_line() {
    let text = edited_sheet(
        "game_of_life.toml",
        &[
            (
                "lines = [\"l1\", \"l2\", \"l3\"]",
                "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
            ),
            (
                "# Lines of Clear",
                "[card.line.l5]\nkind = \"depending_on\"\nproperty = \"status\"\nbranches = { dead = 0, alive = 1, \
                 zombie = 2 }\n\n# Lines of Clear",
            ),
            ("next_line = 5", "next_line = 6"),
        ],
    );
    assert_reported(
        &text,
        Expected::Error,
        "'zombie' is not a state of 'status'",
        "[card.line.l5]",
    );
}

#[test]
fn a_depending_on_a_property_with_no_states_is_one_error() {
    let text = typed_walk("kind = \"depending_on\"\nproperty = \"energy\"\nbranches = { calm = 0, busy = 1 }");
    assert_reported(
        &text,
        Expected::Error,
        "Depending on needs a States property, got 'energy'",
        "[card.line.l3]",
    );
    let report = ParsedSheet::parse(&text).expect("the sheet reads").check();
    let errors = report.errors().count();
    assert_eq!(errors, 1, "{:?}", report.diagnostics);
}

/// Returns the random walk sheet with a property of each type a Set or a comparison reads, and `line` as line 3 of
/// its card.
fn typed_walk(line: &str) -> String {
    let properties = "[[agents.property]]
id = \"heading\"
name = \"Heading\"
type = \"heading\"

[[agents.property]]
id = \"energy\"
name = \"Energy\"
type = \"number\"

[agents.property.starts_as]
rule = \"value\"
value = 0

[[agents.property]]
id = \"whole\"
name = \"Whole\"
type = \"whole_number\"

[agents.property.starts_as]
rule = \"value\"
value = 0

[[agents.property]]
id = \"flag\"
name = \"Flag\"
type = \"yes_no\"

[agents.property.starts_as]
rule = \"value\"
value = false

[[agents.property]]
id = \"status\"
name = \"Status\"
type = \"states\"
states = [
  { id = \"calm\", name = \"Calm\", color = \"#151515\" },
  { id = \"busy\", name = \"Busy\", color = \"#00E676\" },
]

[agents.property.starts_as]
rule = \"value\"
value = { state = \"calm\" }
";
    let text = edited_sheet(
        "random_walk.toml",
        &[
            (
                "[[agents.property]]\nid = \"heading\"\nname = \"Heading\"\ntype = \"heading\"\n",
                properties,
            ),
            ("lines = [\"l1\"]", "lines = [\"l1\", \"l3\"]"),
            ("next_line = 3", "next_line = 4"),
        ],
    );
    format!("{text}\n[card.line.l3]\n{line}\n")
}

#[test]
fn a_value_of_the_wrong_type_is_an_error_at_its_line_as_the_lowering_refuses_it() {
    let set = |property: &str, value: &str| {
        format!(
            "kind = \"set\"\nproperty = \"{property}\"\nvalue = {value}\n\n[card.line.l3.condition]\nkind = \"always\""
        )
    };
    let keep = |property: &str| format!("kind = \"keep_between\"\nproperty = \"{property}\"\nminimum = 0\nmaximum = 1");
    // "Set Energy to 1, when my <left> <test> <right>".
    let compare = |left: &str, test: &str, right: &str| {
        format!(
            "kind = \"set\"\nproperty = \"energy\"\nvalue = 1\n\n[card.line.l3.condition]\nkind = \"compare\"\n\
             test = \"{test}\"\nleft = {{ my = \"{left}\" }}\n{right}"
        )
    };
    let unordered = "states and Boolean values cannot be ordered";
    let cases = [
        (set("energy", "true"), "expected a number in a slot, got a Boolean"),
        (
            set("whole", "{ my = \"flag\" }"),
            "expected a number in a slot, got a Boolean",
        ),
        (set("flag", "3"), "expected a Boolean in a slot, got a number"),
        (
            set("flag", "{ my = \"energy\" }"),
            "expected a Boolean in a slot, got a number",
        ),
        (keep("status"), "Keep between needs a Number or a Vector, got 'status'"),
        (keep("flag"), "Keep between needs a Number or a Vector, got 'flag'"),
        (
            compare("status", "is", "right = 1"),
            "cannot compare a state with a number",
        ),
        (
            compare("energy", "is", "right = { my = \"flag\" }"),
            "cannot compare a number with a Boolean",
        ),
        (
            compare("energy", "one_of", "right = [1, { my = \"status\" }]"),
            "cannot compare a number with a state",
        ),
        (compare("flag", "at_least", "right = true"), unordered),
        (compare("status", "outside", "low = 0\nhigh = 1"), unordered),
        (
            compare("energy", "within", "right = true\ntolerance = 1"),
            "is within compares numbers, got a Boolean",
        ),
        (
            compare("status", "within", "right = 1\ntolerance = 1"),
            "is within compares numbers, got a state",
        ),
    ];
    for (line, message) in cases {
        let text = typed_walk(&line);
        assert_reported(&text, Expected::Error, message, "[card.line.l3]");
        // Each is a value the lowering refuses too.
        let parsed = ParsedSheet::parse(&text).expect("the sheet reads");
        let mut report = parsed.check();
        report
            .diagnostics
            .retain(|diagnostic| diagnostic.diagnostic.severity != DiagnosticSeverity::Error);
        assert!(
            !SheetFile::lowering_problems(&parsed, &report).is_empty(),
            "the lowering takes {line}"
        );
    }
    // A Number takes a yes or no worked out by a line as 1 or 0, and so does "is within".
    for line in [
        set("energy", "{ my = \"flag\" }"),
        compare("flag", "within", "right = 1\ntolerance = 1"),
        compare("whole", "at_least", "right = { my = \"energy\" }"),
        compare("status", "one_of", "right = [{ state = \"busy\" }]"),
    ] {
        let text = typed_walk(&line);
        if let Err(error) = SheetFile::parse(&text) {
            panic!("{line} does not lower: {error}");
        }
    }
}

#[test]
fn a_formula_past_the_depth_limit_is_an_error_at_its_line() {
    let calculate = |formula: &str| {
        edited_sheet(
            "game_of_life.toml",
            &[
                (
                    "lines = [\"l1\", \"l2\", \"l3\"]",
                    "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
                ),
                (
                    "# Lines of Clear",
                    &format!(
                        "[card.line.l5]\nkind = \"calculate\"\nname = \"Sum\"\nformula = \"{formula}\"\n\n# Lines of Clear"
                    ),
                ),
                ("next_line = 5", "next_line = 6"),
            ],
        )
    };
    let sum = |terms: usize| vec!["1"; terms].join(" + ");
    assert_reported(
        &calculate(&sum(MAX_FORMULA_DEPTH + 1)),
        Expected::Error,
        &format!(
            "the formula joins more than {MAX_FORMULA_DEPTH} values in one chain of operations. Group some of them in \
             parentheses"
        ),
        "[card.line.l5]",
    );
    let report = ParsedSheet::parse(&calculate(&sum(MAX_FORMULA_DEPTH)))
        .expect("the sheet reads")
        .check();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
}

#[test]
fn a_set_of_an_arrow_takes_a_place_as_the_lowering_does() {
    // The ants with an Arrow property, Aim, and `line` as line 17 of the ant's card.
    let ants_with = |line: &str| {
        let aim = "[[agents.property]]\nid = \"aim\"\nname = \"Aim\"\ntype = \"arrow\"\n\n\
                   [agents.property.starts_as]\nrule = \"value\"\nkey = 99\n\n[agents.property.starts_as.value]\n\
                   kind = \"random_direction\"\nlength = 1\n\n[[agents.property]]\nid = \"heading\"\n";
        let text = edited_sheet(
            "ants.toml",
            &[
                ("[[agents.property]]\nid = \"heading\"\n", aim),
                ("\"l12\", \"l16\"]", "\"l12\", \"l16\", \"l17\"]"),
                ("next_line = 17", "next_line = 18"),
            ],
        );
        format!("{text}\n[card.line.l17]\n{line}\n")
    };
    // "Set Aim to Food way", and to Ahead, a place picked and a place looked at.
    for place in ["l1", "l6"] {
        let text = ants_with(&format!(
            "kind = \"set\"\nproperty = \"aim\"\nvalue = {{ result = \"{place}\" }}\n\n[card.line.l17.condition]\n\
             kind = \"always\""
        ));
        if let Err(error) = SheetFile::parse(&text) {
            panic!("a Set of Aim to {place} does not build: {error}");
        }
    }
    // A comparison of an arrow with a place is refused by both.
    let text = ants_with(
        "kind = \"set\"\nproperty = \"reward_held\"\nvalue = 1\n\n[card.line.l17.condition]\nkind = \"compare\"\n\
         test = \"is\"\nleft = { my = \"aim\" }\nright = { result = \"l6\" }",
    );
    assert_reported(
        &text,
        Expected::Error,
        "cannot compare a vector with a place",
        "[card.line.l17]",
    );
}
