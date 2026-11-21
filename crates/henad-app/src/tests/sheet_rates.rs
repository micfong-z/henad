//! Tests of rates on the Card pane: the rates SIR rebuilt from Blank by accessible names, the notes of its rate lines
//! with the long tick and with a tried value, the card of a button that offers no rate, the competing lines of the SIRD
//! sheet with "or", their bracket and their group note, a chance changed to a rate and back, the notice under the Time
//! line, the time unit removed and set again from a rate's unit, the parameters that rate and chance pickers offer, a
//! parameter put into a rate taking the rate's unit, and the words rule over every note, menu, picker and notice
//! opened.

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use henad_core::metadata::TimeUnit;
use henad_graph::sheet::ParsedSheet;
use henad_graph::sheet::document::{LineKind, ParameterType, SheetChip, SheetDocument, SheetLiteral, SheetValue, Slot};
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::rates::{chance_from_rate, tick_in};
use henad_graph::sheet::sentence::{
    AddButton, CHANGE_TO_CHANCE, CHANGE_TO_RATE, CHOOSE_WHEN, NEW_PARAMETER, NOT_RUN_TICK, TICK_LENGTH_NAME,
    TICK_UNIT_NAME, chances_per_tick_note, competing_tooltip, long_tick_note, rate_grow_prompt, tick_set_notice,
    tick_unit_choice, unit_mismatch,
};

use henad_graph::sheet::sentence::{Blank, DrawKind, PARAMETERS};

use crate::tests::panes::{
    Seen, add, add_new_outbreak, apply_sheet_edit, assert_controls_hold_their_words, assert_no_forbidden_word,
    build_sir_sections, choose, click, click_on_line, create_parameter, document_of, enter_into, is_open, line_text,
    open_starter, pick, picker_labels, picker_offers, sheet, shows, shows_notice, start_infected_by_chance,
    starter_document, text_and_code, type_and_enter,
};
use crate::tests::support::{Harness, SIR_CHANCE, align_with_starter, edited, settle_preview, starter_text};
use crate::ui::editor::sheet::SheetEditor;
use crate::ui::editor::sheet::card_pane::competing_brackets;
use crate::ui::params::{TrialChoice, apply_trial};

/// Text of the SIRD sheet, a copy of henad-graph's test sheet: Recovered and Dead leave Infected at competing rates.
const SIRD: &str = include_str!("sheets/sird.toml");

/// Note under line 2 of the rates SIR at its defaults and one tick of 1 day.
const INFECTION_NOTE: &str = "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259 with 1 infected \
                              neighbour, and 0.451 with 2. The rate is converted to a chance by 1 − e^(−0.3 × n × 1 \
                              day), where n is Infected neighbours.";

/// Note under line 3 of the rates SIR at its defaults and one tick of 1 day.
const RECOVERY_NOTE: &str = "Each tick (1 day), an Infected cell becomes Recovered with chance 0.0952. The rate is \
                             converted to a chance by 1 − e^(−0.1 × 1 day).";

/// Returns the number of nodes of a settled frame whose label or value starts with `start`, leaving out the text run
/// under each label.
fn count_starting(harness: &mut Harness, start: &str) -> usize {
    harness
        .settle()
        .iter()
        .filter(|node| node.role != Role::TextRun && (node.label.starts_with(start) || node.value.starts_with(start)))
        .count()
}

/// Opens the menu of the line whose prefix is `prefix`, records what it shows, and returns whether it offers `item`.
fn menu_offers(harness: &mut Harness, seen: &mut Seen, prefix: &str, item: &str) -> bool {
    click(harness, &format!("{prefix} actions"));
    seen.look(harness);
    let offered = shows(harness, item);
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    offered
}

/// Returns the labels of the parameters that the picker of a `blank` on line 3 of `document` offers.
fn parameters_offered(document: &SheetDocument, blank: Blank) -> Vec<String> {
    picker_labels(document, "l3", blank, true, DrawKind::Rate)
        .into_iter()
        .filter(|(heading, _)| heading == PARAMETERS)
        .map(|(_, label)| label)
        .collect()
}

/// Returns the draw of the condition of line `line` of the sheet in the harness: its kind and its typed number.
fn draw_of(harness: &Harness, line: &str) -> (LineKind, Option<f64>) {
    let (_, line) = sheet(harness).document().line(line).expect("the line");
    let Some(SheetValue::Phrase(phrase)) = line.condition.value() else {
        panic!("the line has no draw: {line:?}");
    };
    let slot = if phrase.kind == LineKind::Rate {
        &phrase.rate
    } else {
        &phrase.chance
    };
    let typed = match slot {
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(number))) => Some(*number),
        _ => None,
    };
    (phrase.kind, typed)
}

#[test]
fn a_blank_model_builds_the_rates_sir_with_its_lines_and_equals_the_starter() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    let starter = starter_document("sir.toml");
    build_sir_sections(&mut harness, &mut seen, "SIR", &starter, Some(TimeUnit::Day));

    // Line 2 starts in the tick's unit, takes a new rate parameter "per day", and grows for each infected neighbour
    // from the note that asks.
    add(&mut harness, &mut seen, AddButton::Chance, "Become, at a rate");
    pick(&mut harness, &mut seen, "Choose a state", "Infected");
    pick(&mut harness, &mut seen, "Choose the old state", "Susceptible");
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Infected from Susceptible, at rate ... per day"
    );
    click(&mut harness, "Line 2: choose a rate");
    pick(&mut harness, &mut seen, "Choose a rate", NEW_PARAMETER);
    create_parameter(&mut harness, &mut seen, "Infection rate", "0.3", Some("5"));
    assert!(shows(&mut harness, &rate_grow_prompt("Infected neighbours")));
    seen.look(&mut harness);
    click(&mut harness, "Line 2: add for each infected neighbour");
    harness.settle();
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Infected from Susceptible, at rate Infection rate per day for each infected neighbour"
    );
    assert!(shows(&mut harness, INFECTION_NOTE), "line 2 has its rate note");
    assert!(!shows(&mut harness, &rate_grow_prompt("Infected neighbours")));

    add(&mut harness, &mut seen, AddButton::Chance, "Become, at a rate");
    pick(&mut harness, &mut seen, "Choose a state", "Recovered");
    pick(&mut harness, &mut seen, "Choose the old state", "Infected");
    click(&mut harness, "Line 3: choose a rate");
    assert!(picker_offers(&mut harness, "Choose a rate", "Infection rate"));
    pick(&mut harness, &mut seen, "Choose a rate", NEW_PARAMETER);
    create_parameter(&mut harness, &mut seen, "Recovery rate", "0.1", Some("5"));
    assert_eq!(
        line_text(&mut harness, "Line 3"),
        "Become Recovered from Infected, at rate Recovery rate per day"
    );
    assert!(shows(&mut harness, RECOVERY_NOTE), "line 3 has its rate note");
    seen.look(&mut harness);

    start_infected_by_chance(&mut harness, &mut seen);
    add_new_outbreak(&mut harness, &mut seen);

    let aligned = align_with_starter(sheet(&harness).document(), &starter);
    assert_eq!(
        text_and_code(&aligned),
        text_and_code(&starter),
        "the rebuild equals the SIR starter in canonical text and lowered code"
    );
    assert_controls_hold_their_words(&mut harness);
    click(&mut harness, "Help: lines");
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);
}

#[test]
fn the_run_once_card_offers_no_rate_after_rates_arrive() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "sir.toml");
    assert!(shows(&mut harness, INFECTION_NOTE));
    assert!(shows(&mut harness, RECOVERY_NOTE));
    assert!(
        shows(
            &mut harness,
            "On average a cell stays Infected 10.5 days at this tick length. It approaches 1/0.1 = 10 days as the tick \
             shrinks."
        ),
        "line 3's note describes the mean stay"
    );
    seen.look(&mut harness);

    // A line that runs each tick offers the rate in "Choose when", and a typed rate changes to a chance.
    click(&mut harness, "Line 3: change the kind of condition");
    assert!(is_open(&mut harness, CHOOSE_WHEN));
    assert!(shows(&mut harness, "at rate ... per ..."));
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    assert!(
        !menu_offers(&mut harness, &mut seen, "Line 3", CHANGE_TO_CHANCE),
        "a parameter is no typed number"
    );
    click_on_line(&mut harness, "Line 3", "Recovery rate");
    assert!(is_open(&mut harness, "Choose a rate"));
    type_and_enter(&mut harness, "0.1");
    assert!(menu_offers(&mut harness, &mut seen, "Line 3", CHANGE_TO_CHANCE));

    // The card of New outbreak offers no rate, in "Choose when" or in its line menu, even for a typed chance.
    click(&mut harness, "New outbreak line 1: change the kind of condition");
    assert!(is_open(&mut harness, CHOOSE_WHEN));
    assert!(!shows(&mut harness, "at rate ... per ..."));
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    click_on_line(&mut harness, "New outbreak line 1", "Initial infected");
    assert!(is_open(&mut harness, "Choose a chance"));
    type_and_enter(&mut harness, "0.01");
    assert_eq!(
        line_text(&mut harness, "New outbreak line 1"),
        "Become Infected from Susceptible, with chance 0.01"
    );
    assert!(!menu_offers(
        &mut harness,
        &mut seen,
        "New outbreak line 1",
        CHANGE_TO_RATE
    ));
    assert_no_forbidden_word(&seen);
}

#[test]
fn the_sird_lines_3_and_4_read_or_with_one_bracket_and_one_group_note() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    harness.open_sheet(SIRD);
    assert_eq!(
        line_text(&mut harness, "Line 3"),
        "Become Recovered from Infected, at rate Recovery rate per day"
    );
    assert_eq!(
        line_text(&mut harness, "Line 4"),
        "or become Dead from Infected, at rate Death rate per day"
    );
    assert_eq!(competing_brackets(sheet(&harness).document(), 0), [vec![3, 4]]);
    assert_eq!(
        count_starting(
            &mut harness,
            "Each tick (1 day), an Infected cell leaves Infected with chance"
        ),
        1,
        "the group has one note"
    );
    assert_eq!(
        count_starting(&mut harness, "Each tick (1 day), an Infected cell becomes"),
        0,
        "the group's note replaces the lines' own notes"
    );
    assert!(
        shows(&mut harness, &competing_tooltip(&[3, 4], "Infected", "cell")),
        "the word or describes the group"
    );
    seen.look(&mut harness);
    assert_controls_hold_their_words(&mut harness);
    assert_no_forbidden_word(&seen);
}

#[test]
fn change_to_a_rate_and_back_keeps_the_chance_per_tick() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    harness.open_sheet(SIR_CHANCE);
    click_on_line(&mut harness, "Line 3", "Recovery chance");
    assert!(is_open(&mut harness, "Choose a chance"));
    type_and_enter(&mut harness, "0.05");
    assert!(
        !menu_offers(&mut harness, &mut seen, "Line 3", CHANGE_TO_RATE),
        "one tick has no length"
    );

    // A new length of one tick names the lines that use chances per tick under the Time line.
    choose(&mut harness, TICK_UNIT_NAME, tick_unit_choice(Some(TimeUnit::Day)));
    assert!(shows_notice(&mut harness, &chances_per_tick_note(&[2, 3])));
    seen.look(&mut harness);

    assert!(menu_offers(&mut harness, &mut seen, "Line 3", CHANGE_TO_RATE));
    click(&mut harness, "Line 3 actions");
    click(&mut harness, CHANGE_TO_RATE);
    harness.settle();
    let (kind, rate) = draw_of(&harness, "l3");
    assert_eq!(kind, LineKind::Rate);
    assert!(
        shows_notice(&mut harness, &chances_per_tick_note(&[2])),
        "the notice under the Time line follows the card"
    );
    assert!(!shows_notice(&mut harness, &chances_per_tick_note(&[2, 3])));
    let rate = rate.expect("a typed rate");
    let time = sheet(&harness).document().time.expect("one tick has a length");
    let chance = chance_from_rate(rate, tick_in(time, TimeUnit::Day));
    assert!(
        (chance - 0.05).abs() < 1e-7,
        "the rate {rate} keeps the chance 0.05 per tick, not {chance}"
    );
    assert!(
        shows(
            &mut harness,
            &format!(
                "Each tick (1 day), an Infected cell becomes Recovered with chance 0.05. The rate is converted to a \
                 chance by 1 − e^(−{} × 1 day).",
                henad_graph::sheet::format_number(rate).expect("a number")
            )
        ),
        "the rate's note states the same chance"
    );
    seen.look(&mut harness);

    assert!(menu_offers(&mut harness, &mut seen, "Line 3", CHANGE_TO_CHANCE));
    click(&mut harness, "Line 3 actions");
    click(&mut harness, CHANGE_TO_CHANCE);
    harness.settle();
    assert_eq!(draw_of(&harness, "l3"), (LineKind::Chance, Some(0.05)));
    assert!(shows_notice(&mut harness, &chances_per_tick_note(&[2, 3])));
    assert_eq!(
        line_text(&mut harness, "Line 3"),
        "Become Recovered from Infected, with chance 0.05"
    );
    assert_no_forbidden_word(&seen);
}

#[test]
fn the_long_tick_note_shows_at_one_day_and_leaves_at_a_quarter_day() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "sir.toml");
    let long = long_tick_note("up to 0.909 with 8");
    assert!(shows(&mut harness, &long), "1 − e^(−0.3 × 8) is above one half");
    enter_into(&mut harness, TICK_LENGTH_NAME, "0.25");
    assert!(!shows(&mut harness, &long));
    assert_eq!(
        count_starting(&mut harness, "Up to "),
        0,
        "no line has a long tick at a quarter day"
    );
    assert!(shows(
        &mut harness,
        "Each tick (0.25 days), an Infected cell becomes Recovered with chance 0.0247. The rate is converted to a \
         chance by 1 − e^(−0.1 × 0.25 days)."
    ));
}

#[test]
fn removing_the_time_unit_marks_the_rate_lines_and_a_rate_unit_sets_it_again() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "sir.toml");
    assert_eq!(count_starting(&mut harness, NOT_RUN_TICK), 0);
    choose(&mut harness, TICK_UNIT_NAME, tick_unit_choice(None));
    assert_eq!(sheet(&harness).document().time, None);
    assert_eq!(
        count_starting(&mut harness, NOT_RUN_TICK),
        2,
        "lines 2 and 3 do not run"
    );
    assert!(!shows(&mut harness, RECOVERY_NOTE), "a rate with no tick has no note");
    seen.look(&mut harness);

    // A unit chosen on a rate's line while one tick has no length sets one tick to 1 of it, with a notice.
    click(&mut harness, "Line 3: unit of time");
    seen.look(&mut harness);
    click(&mut harness, "week");
    harness.settle();
    let time = sheet(&harness).document().time.expect("one tick has a length again");
    assert_eq!((time.tick_length, time.tick_unit), (1.0, TimeUnit::Week));
    assert!(shows_notice(&mut harness, &tick_set_notice(TimeUnit::Week)));
    assert_eq!(count_starting(&mut harness, NOT_RUN_TICK), 0);
    assert!(
        shows(
            &mut harness,
            &unit_mismatch("Recovery rate", TimeUnit::Day, TimeUnit::Week)
        ),
        "line 3 reads its parameter per week"
    );
    seen.look(&mut harness);

    // A new length of one tick on the sheet leaves the notice with a length the sheet no longer has.
    enter_into(&mut harness, TICK_LENGTH_NAME, "2");
    assert_eq!(sheet(&harness).document().time.map(|time| time.tick_length), Some(2.0));
    assert!(!shows_notice(&mut harness, &tick_set_notice(TimeUnit::Week)));
    assert_no_forbidden_word(&seen);
}

#[test]
fn a_rate_picker_leaves_out_a_parameter_of_another_unit_and_a_chance_picker_every_rate_parameter() {
    let text = edited(
        starter_text("sir.toml"),
        "[[parameter]]",
        "[[parameter]]\nid = \"patch_size\"\nname = \"Patch size\"\ntype = \"number\"\nmin = 0\nmax = 10\n\
             default = 3\nstep = 1\nunit = \"cells\"\n\n[[parameter]]\nid = \"scale\"\nname = \"Scale\"\n\
             type = \"number\"\nmin = 0\nmax = 1\ndefault = 0.5\nstep = 0.1\n\n[[parameter]]\nid = \"drift\"\n\
             name = \"Drift\"\ntype = \"number\"\nmin = -1\nmax = 1\ndefault = 0\nstep = 0.1\n\n[[parameter]]",
    );
    let document = document_of(&text);
    assert_eq!(
        parameters_offered(&document, Blank::Rate),
        ["Scale", "Infection rate", "Recovery rate", "Initial infected"],
        "a rate's picker keeps the parameters per unit of time and those with no Unit, and leaves out Drift, whose Min \
         is below 0"
    );
    assert_eq!(
        parameters_offered(&document, Blank::Chance),
        ["Scale", "Initial infected"],
        "a chance's picker leaves out the parameters per unit of time, and Patch size, whose range leaves 0 to 1"
    );
}

#[test]
fn the_rate_notes_read_a_tried_value_and_the_default_after_reset() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "sir.toml");
    settle_preview(&mut harness.app);
    harness.app.wait_for_build();
    assert!(harness.app.previews_editor_sheet(), "the sheet previews");
    assert!(shows(&mut harness, INFECTION_NOTE));

    let entry = harness.app.loaded_entry().expect("a preview").clone();
    let index = entry
        .param_descriptors()
        .iter()
        .position(|descriptor| descriptor.id() == "infection_rate")
        .expect("the rate");
    apply_trial(
        &mut harness.app,
        &entry,
        index,
        TrialChoice::Try(henad_core::params::ParamValue::F32(0.5)),
    );
    let tried = "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.393 with 1 infected neighbour, \
                 and 0.632 with 2. The rate is converted to a chance by 1 − e^(−0.5 × n × 1 day), where n is Infected \
                 neighbours.";
    assert!(shows(&mut harness, tried), "line 2's note reads the tried value");
    assert!(!shows(&mut harness, INFECTION_NOTE));

    apply_trial(&mut harness.app, &entry, index, TrialChoice::Reset);
    assert!(
        shows(&mut harness, INFECTION_NOTE),
        "line 2's note reads the default again"
    );
    assert!(!shows(&mut harness, tried));
}

/// A parameter with no unit put into a rate takes the rate's unit in the same edit, and one Undo takes both back.
#[test]
fn a_parameter_put_into_a_rate_takes_its_unit_and_one_undo_restores_both() {
    let parsed = ParsedSheet::parse(starter_text("sir.toml")).expect("the starter reads");
    let mut sheet = SheetEditor::new(parsed.document().clone());
    apply_sheet_edit(
        &mut sheet,
        SheetEdit::AddParameter {
            name: "Plain".to_owned(),
            parameter_type: ParameterType::Number,
            min: 0.0,
            max: 1.0,
            default: 0.5,
            unit: None,
        },
    );
    let before = sheet.document().clone();
    apply_sheet_edit(
        &mut sheet,
        SheetEdit::SetSlot {
            line: "l3".to_owned(),
            path: "condition.rate".to_owned(),
            value: Slot::Filled(SheetValue::Chip(SheetChip::Param("plain".to_owned()))),
        },
    );
    let plain_unit = |document: &SheetDocument| {
        document
            .parameters
            .iter()
            .find(|parameter| parameter.id == "plain")
            .and_then(|parameter| parameter.unit.clone())
    };
    assert_eq!(plain_unit(sheet.document()).as_deref(), Some("per day"));
    assert!(sheet.undo(false), "one edit to undo");
    assert_eq!(sheet.document(), &before, "one Undo restores the rate and the unit");
    assert_eq!(plain_unit(sheet.document()), None);
}

/// A new name typed for a result and left by a click on a note's button in one frame is kept, and so is the button's
/// edit.
#[test]
fn a_new_result_name_and_a_note_button_in_one_frame_both_commit() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let text = edited(starter_text("sir.toml"), "tries = { result = \"l1\" }\n", "");
    harness.open_sheet(&text);
    assert!(shows(&mut harness, &rate_grow_prompt("Infected neighbours")));
    click(&mut harness, "Line 1 actions");
    click(&mut harness, "Rename line 1");
    harness.replace_text("Sick nearby");
    click(&mut harness, "Line 2: add for each infected neighbour");
    // The field reports losing the focus again on the next frame. Only the frame of the click shows a dropped name.
    let (_, count) = sheet(&harness).document().line("l1").expect("line 1");
    assert_eq!(
        count.name.as_deref(),
        Some("Sick nearby"),
        "the typed name is kept in the frame of the click"
    );
    harness.settle();
    let document = sheet(&harness).document();
    let (_, become_line) = document.line("l2").expect("line 2");
    let Some(SheetValue::Phrase(rate)) = become_line.condition.value() else {
        panic!("line 2 grows at a rate");
    };
    assert!(matches!(rate.tries, Slot::Filled(_)), "the button's edit is kept");
}
