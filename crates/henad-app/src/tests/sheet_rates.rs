//! Tests of rates on the Card pane: the rates SIR rebuilt from Blank by accessible names, the notes of its rate lines
//! with the long tick and with a tried value, the card of a button that offers no rate, the competing lines of the SIRD
//! sheet with "or", their bracket and their group note, a chance changed to a rate and back, the notice under the Time
//! line, the time unit removed and set again from a rate's unit, the parameters that rate and chance pickers offer, and
//! the words rule over every note, menu, picker and notice opened.

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use henad_core::metadata::TimeUnit;
use henad_graph::sheet::document::{LineKind, SheetDocument, SheetLiteral, SheetValue, Slot};
use henad_graph::sheet::rates::{chance_from_rate, tick_in};
use henad_graph::sheet::sentence::{
    ADD_BUTTON, AddButton, BUTTON_NAME_FIELD, CHANGE_TO_CHANCE, CHANGE_TO_RATE, CHOOSE_WHEN, DONE, NEW_PARAMETER,
    NOT_RUN_TICK, TICK_LENGTH_NAME, TICK_UNIT_NAME, chances_per_tick_note, competing_tooltip, long_tick_note,
    rate_grow_prompt, run_once, tick_set_notice, tick_unit_choice, unit_mismatch,
};

use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{Blank, DrawKind, PARAMETERS, Sentences};
use henad_graph::sheet::starters;
use web_time::Instant;

use super::sheet_card::{
    Seen, TALL_SCREEN, add, add_states_property, assert_controls_hold_their_words, assert_no_forbidden_word, choose,
    click, click_on_line, create_parameter, document_of, enter_into, is_open, line_text, new_blank, open_starter, pick,
    sheet, shows, starter_document, text_and_code, type_and_enter, type_into,
};
use crate::tests::support::{Harness, align_with_starter};
use crate::ui::editor::preview::{DEBOUNCE, poll_at};
use crate::ui::editor::sheet::card_pane::competing_brackets;
use crate::ui::editor::sheet::picker::{LineContext, picker_for};
use crate::ui::params::{TrialChoice, apply_trial};

/// Text of the chance SIR, a copy of the sheet henad-graph's and henad-models' tests read.
const SIR_CHANCE: &str = include_str!("sheets/sir_chance.toml");

/// Text of the SIRD sheet, a copy of henad-graph's test sheet: Recovered and Dead leave Infected at competing rates.
const SIRD: &str = include_str!("sheets/sird.toml");

/// Note under line 2 of the rates SIR at its defaults and one tick of 1 day.
const INFECTION_NOTE: &str = "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259 with 1 infected \
                              neighbour, and 0.451 with 2. A rate is not a chance. Henad turns it into one as 1 − \
                              e^(−0.3 × n × 1 day), where n is Infected neighbours.";

/// Note under line 3 of the rates SIR at its defaults and one tick of 1 day.
const RECOVERY_NOTE: &str = "Each tick (1 day), an Infected cell becomes Recovered with chance 0.0952. A rate is not a \
                             chance. Henad turns it into one as 1 − e^(−0.1 × 1 day).";

/// Returns the number of nodes of a settled frame whose label or value starts with `start`, leaving out the text run
/// under each label.
fn count_starting(harness: &mut Harness, start: &str) -> usize {
    harness
        .settle()
        .iter()
        .filter(|node| node.role != Role::TextRun && (node.label.starts_with(start) || node.value.starts_with(start)))
        .count()
}

/// Returns whether a settled frame shows the notice `notice` after its icon.
fn shows_notice(harness: &mut Harness, notice: &str) -> bool {
    harness
        .accessible_texts()
        .iter()
        .any(|text| text.strip_suffix(notice).is_some_and(|icon| icon.ends_with(' ')))
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

/// Returns whether the open picker titled `dialog` offers an item named `item`.
fn picker_offers(harness: &mut Harness, dialog: &str, item: &str) -> bool {
    let nodes = harness.settle();
    let area = nodes
        .iter()
        .find(|node| node.role == Role::Dialog && node.label == dialog)
        .and_then(|node| node.bounds)
        .unwrap_or_else(|| panic!("{dialog} is open"));
    nodes
        .iter()
        .any(|node| node.label == item && node.bounds.is_some_and(|rect| area.contains(rect.center())))
}

/// Returns the labels of the parameters that the picker of a `blank` on line 3 of `document`'s first card offers.
fn picker_labels(document: &SheetDocument, blank: Blank) -> Vec<String> {
    let sentences = Sentences::new(document);
    let relations = CardRelations::new(document, &document.cards[0]);
    let (_, line) = document.line("l3").expect("line 3");
    let context = LineContext {
        document,
        sentences: &sentences,
        relations: &relations,
        line_id: "l3",
        line,
    };
    picker_for(&context, blank, None, true, "", DrawKind::Rate)
        .groups
        .iter()
        .filter(|group| group.heading == PARAMETERS)
        .flat_map(|group| group.items.iter().map(|item| item.label.clone()))
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

/// Adds the button New outbreak, whose card makes a Susceptible cell Infected with chance Initial infected, and
/// checks that its card offers no rate and its chance picker no rate parameter.
fn add_new_outbreak(harness: &mut Harness, seen: &mut Seen) {
    click(harness, ADD_BUTTON);
    type_into(harness, BUTTON_NAME_FIELD, "New outbreak");
    click(harness, &run_once("cell"));
    click(harness, DONE);
    harness.settle();
    add(harness, seen, AddButton::Change, "Become");
    pick(harness, seen, "Choose a state", "Infected");
    pick(harness, seen, "Choose the old state", "Susceptible");
    click(harness, "New outbreak line 1: choose when");
    assert!(is_open(harness, CHOOSE_WHEN));
    assert!(!shows(harness, "at rate ... per ..."), "a button's card takes no rate");
    pick(harness, seen, CHOOSE_WHEN, "with chance ...");
    click(harness, "New outbreak line 1: choose a chance");
    assert!(picker_offers(harness, "Choose a chance", "Initial infected"));
    for rate in ["Infection rate", "Recovery rate"] {
        assert!(
            !picker_offers(harness, "Choose a chance", rate),
            "a chance's picker leaves out the rate parameter {rate}"
        );
    }
    pick(harness, seen, "Choose a chance", "Initial infected");
}

#[test]
fn a_blank_model_builds_the_rates_sir_with_its_lines_and_equals_the_starter() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    let mut seen = Seen::default();
    let starter = starter_document("sir.toml");
    // The id comes from the name given in the New model dialog, and the toolbar renames the model keeping it.
    new_blank(&mut harness, "SIR");
    click(&mut harness, "Model name: SIR. Rename");
    harness.settle();
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![egui::Event::Text("SIR epidemic".to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
    enter_into(&mut harness, "Width in cells", "1024");
    enter_into(&mut harness, "Height in cells", "1024");
    choose(&mut harness, TICK_UNIT_NAME, tick_unit_choice(Some(TimeUnit::Day)));
    add_states_property(&mut harness, "Health", &["Susceptible", "Infected", "Recovered"]);
    click(&mut harness, DONE);
    harness.settle();
    for state in ["Susceptible", "Infected", "Recovered"] {
        click(&mut harness, &format!("Plot {state}"));
    }
    type_into(
        &mut harness,
        "Purpose",
        starter.sheet.purpose.as_deref().unwrap_or_default(),
    );
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();

    add(&mut harness, &mut seen, AddButton::LookAround, "Count");
    pick(&mut harness, &mut seen, "Choose a property", "Health");
    pick(&mut harness, &mut seen, "Choose a state", "Infected");

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

    choose(&mut harness, "Health starts as", "By chance");
    click(&mut harness, "Health starts as, row 1: use a parameter");
    click(&mut harness, NEW_PARAMETER);
    create_parameter(&mut harness, &mut seen, "Initial infected", "0.01", Some("1"));

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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
                "Each tick (1 day), an Infected cell becomes Recovered with chance 0.05. A rate is not a chance. Henad \
                 turns it into one as 1 − e^(−{} × 1 day).",
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
        "Each tick (0.25 days), an Infected cell becomes Recovered with chance 0.0247. A rate is not a chance. Henad \
         turns it into one as 1 − e^(−0.1 × 0.25 days)."
    ));
}

#[test]
fn removing_the_time_unit_marks_the_rate_lines_and_a_rate_unit_sets_it_again() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let text = starters()
        .iter()
        .find(|starter| starter.file_name == "sir.toml")
        .expect("the starter")
        .text
        .replacen(
            "[[parameter]]",
            "[[parameter]]\nid = \"patch_size\"\nname = \"Patch size\"\ntype = \"number\"\nmin = 0\nmax = 10\n\
             default = 3\nstep = 1\nunit = \"cells\"\n\n[[parameter]]\nid = \"scale\"\nname = \"Scale\"\n\
             type = \"number\"\nmin = 0\nmax = 1\ndefault = 0.5\nstep = 0.1\n\n[[parameter]]",
            1,
        );
    let document = document_of(&text);
    assert_eq!(
        picker_labels(&document, Blank::Rate),
        ["Scale", "Infection rate", "Recovery rate", "Initial infected"],
        "a rate's picker keeps the parameters per unit of time and those with no Unit"
    );
    assert_eq!(
        picker_labels(&document, Blank::Chance),
        ["Patch size", "Scale", "Initial infected"],
        "a chance's picker leaves out the parameters per unit of time"
    );
}

#[test]
fn the_rate_notes_read_a_tried_value_and_the_default_after_reset() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    open_starter(&mut harness, "sir.toml");
    let now = Instant::now();
    poll_at(&harness.context, &mut harness.app, now);
    poll_at(&harness.context, &mut harness.app, now + DEBOUNCE);
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
    let tried = "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.393 with 1 infected neighbour, and \
                 0.632 with 2. A rate is not a chance. Henad turns it into one as 1 − e^(−0.5 × n × 1 day), where n is \
                 Infected neighbours.";
    assert!(shows(&mut harness, tried), "line 2's note reads the tried value");
    assert!(!shows(&mut harness, INFECTION_NOTE));

    apply_trial(&mut harness.app, &entry, index, TrialChoice::Reset);
    assert!(
        shows(&mut harness, INFECTION_NOTE),
        "line 2's note reads the default again"
    );
    assert!(!shows(&mut harness, tried));
}
