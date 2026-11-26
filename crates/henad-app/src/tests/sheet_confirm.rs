//! Tests of the dialog that asks before an edit of the sheet takes away parts of it besides the item it deletes or
//! changes: the deletes, the property form's new kind and removed states, a new start, new colours and the buttons.
//! Each asks with what goes, Cancel keeps the sheet, the action makes the edit as one step of Undo, and an edit that
//! takes nothing else away asks nothing.

use henad_graph::sheet::document::{ButtonStepRule, ColorRuleKind, PropertyType, StartRule};
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::sentence::{
    ADD_STATE, CANCEL, CHANGE, CHANGE_BUTTON_HEADING, CHANGE_COLORS_HEADING, CHANGE_KIND, CHANGE_KIND_HEADING,
    COLOR_AGENTS_BY, COLOR_GROUND_BY, DELETE, DELETE_BUTTON_HEADING, DELETE_EVENT_HEADING, DELETE_PARAMETER_HEADING,
    DELETE_PROPERTY, DELETE_PROPERTY_HEADING, DELETE_STATE_HEADING, DELETE_STATES_HEADING, DONE, NUMBER_KIND, REMOVE,
    REMOVE_MEASURE_HEADING, REPLACE, REPLACE_START_HEADING, START_AGAIN, UNDO,
};

use crate::tests::panes::{
    Seen, apply_sheet_edit, assert_no_forbidden_word, choose, click, field_value, is_open, open_starter,
    press_in_dialog, question_in, sheet, sheet_mut, shows, text, type_into,
};
use crate::tests::support::Harness;

/// Asserts that the dialog titled `title` asks `question`, records what the frame shows in `seen`, presses Cancel and
/// asserts that the sheet still reads `before`.
fn ask_and_cancel(harness: &mut Harness, seen: &mut Seen, title: &str, question: &str, before: &str) {
    assert_eq!(question_in(harness, title).as_deref(), Some(question), "{title}");
    seen.look(harness);
    press_in_dialog(harness, title, CANCEL);
    assert!(!is_open(harness, title));
    assert_eq!(text(harness), before, "Cancel keeps the sheet");
}

/// Presses `action` in the dialog titled `title`, asserts that `kept` holds of the sheet, then presses Undo once and
/// asserts that the sheet reads `before` again.
fn act_and_undo(harness: &mut Harness, title: &str, action: &str, kept: impl Fn(&Harness) -> bool, before: &str) {
    press_in_dialog(harness, title, action);
    assert!(!is_open(harness, title));
    assert!(kept(harness), "{action} makes the edit");
    click(harness, UNDO);
    assert_eq!(text(harness), before, "one Undo takes the edit back");
}

/// Returns whether the cells of the sheet in `harness` hold a property named `name`.
fn has_cell_property(harness: &Harness, name: &str) -> bool {
    sheet(harness)
        .document()
        .cells
        .iter()
        .flatten()
        .any(|property| property.name == name)
}

/// Returns the states of SIR's Health in the sheet in `harness`, by name.
fn health_states(harness: &Harness) -> Vec<String> {
    let document = sheet(harness).document();
    let health = document.cells.iter().flatten().find(|property| property.id == "health");
    health
        .map(|health| health.states.iter().map(|state| state.name.clone()).collect())
        .unwrap_or_default()
}

/// Deleting SIR's Health, removing its state Infected in the property form (by its × or by clearing its name), removing
/// two states, or giving Health another kind asks first and names the lines, the start, the measures and the colour
/// rule that go. The form stays open after Cancel, the action
/// makes the edit as one step of Undo, and a rename or a new state asks nothing.
#[test]
fn deleting_or_retyping_a_property_or_removing_a_state_asks_with_what_goes() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "sir.toml");
    let starter = text(&harness);
    let health_lost = "Lines 1, 2 and 3 and New outbreak line 1 will lose Health. The measures Susceptible, Infected and \
                       Recovered will be removed. The cells will no longer be coloured by Health.";

    click(&mut harness, "Edit property Health");
    click(&mut harness, DELETE_PROPERTY);
    ask_and_cancel(&mut harness, &mut seen, DELETE_PROPERTY_HEADING, health_lost, &starter);
    assert!(shows(&mut harness, DONE), "the property form stays open");
    click(&mut harness, DELETE_PROPERTY);
    act_and_undo(
        &mut harness,
        DELETE_PROPERTY_HEADING,
        DELETE,
        |harness| !has_cell_property(harness, "Health"),
        &starter,
    );

    click(&mut harness, "Edit property Health");
    click(&mut harness, "Remove state 2");
    click(&mut harness, DONE);
    let infected_lost = "Lines 1, 2 and 3, New outbreak line 1 and the start of Health will lose Infected. The measure \
                         Infected will be removed.";
    ask_and_cancel(&mut harness, &mut seen, DELETE_STATE_HEADING, infected_lost, &starter);
    assert_eq!(
        field_value(&mut harness, "State 2 name"),
        "Recovered",
        "the form keeps its rows"
    );
    click(&mut harness, DONE);
    act_and_undo(
        &mut harness,
        DELETE_STATE_HEADING,
        DELETE,
        |harness| health_states(harness) == ["Susceptible", "Recovered"],
        &starter,
    );

    click(&mut harness, "Edit property Health");
    click(&mut harness, NUMBER_KIND);
    click(&mut harness, DONE);
    ask_and_cancel(&mut harness, &mut seen, CHANGE_KIND_HEADING, health_lost, &starter);
    click(&mut harness, DONE);
    let retyped = |harness: &Harness| {
        let document = sheet(harness).document();
        document
            .cells
            .iter()
            .flatten()
            .next()
            .map(|property| property.property_type)
            == Some(PropertyType::Number)
    };
    act_and_undo(&mut harness, CHANGE_KIND_HEADING, CHANGE_KIND, retyped, &starter);

    // A state whose name is cleared goes as a removed one does, and two removed states ask together.
    click(&mut harness, "Edit property Health");
    click(&mut harness, "State 2 name");
    harness.press(egui::Key::A, egui::Modifiers::COMMAND);
    harness.press(egui::Key::Backspace, egui::Modifiers::NONE);
    click(&mut harness, DONE);
    ask_and_cancel(&mut harness, &mut seen, DELETE_STATE_HEADING, infected_lost, &starter);
    click(&mut harness, "Remove state 3");
    click(&mut harness, ADD_STATE);
    type_into(&mut harness, "State 3 name", "Exposed");
    click(&mut harness, DONE);
    let two_lost = "Lines 1, 2 and 3, New outbreak line 1 and the start of Health will lose Infected and Recovered. \
                    The measures Infected and Recovered will be removed.";
    ask_and_cancel(&mut harness, &mut seen, DELETE_STATES_HEADING, two_lost, &starter);
    click(&mut harness, CANCEL);

    // A rename and a new state take nothing away.
    click(&mut harness, "Edit property Health");
    type_into(&mut harness, "Property name", "Condition");
    click(&mut harness, ADD_STATE);
    type_into(&mut harness, "State 4 name", "Exposed");
    click(&mut harness, DONE);
    for title in [DELETE_STATE_HEADING, CHANGE_KIND_HEADING] {
        assert!(!is_open(&mut harness, title), "{title} asks");
    }
    assert!(has_cell_property(&harness, "Condition"));
    click(&mut harness, UNDO);
    assert_eq!(text(&harness), starter);

    // A parameter that a start reads.
    click(&mut harness, "Delete parameter Initial infected");
    let initial_lost = "New outbreak line 1 and the start of Health will lose Initial infected.";
    ask_and_cancel(
        &mut harness,
        &mut seen,
        DELETE_PARAMETER_HEADING,
        initial_lost,
        &starter,
    );
    assert_no_forbidden_word(&seen);
}

/// A new start of the ants' Ground drops its painted shapes and leaves the ants no Nest to start on, and new colours of
/// the ground drop its shades, which no control makes again. Both ask first. A new start or new colours that drop
/// nothing ask nothing.
#[test]
fn a_new_start_or_new_colours_ask_before_they_drop_what_no_control_makes() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "ants.toml");
    let starter = text(&harness);
    let ground_rule = |harness: &Harness, rule: StartRule| {
        let document = sheet(harness).document();
        let ground = document.ground.as_ref().and_then(|ground| ground.properties.first());
        ground
            .and_then(|ground| ground.starts_as.as_ref())
            .map(|start| start.rule)
            == Some(rule)
    };

    choose(&mut harness, "Ground starts as", "All Open");
    let painted_lost = "The 4 shapes painted on Ground will be lost. Position will have no Nest cell to start on.";
    ask_and_cancel(&mut harness, &mut seen, REPLACE_START_HEADING, painted_lost, &starter);
    choose(&mut harness, "Ground starts as", "All Open");
    act_and_undo(
        &mut harness,
        REPLACE_START_HEADING,
        REPLACE,
        |harness| ground_rule(harness, StartRule::Value),
        &starter,
    );

    click(&mut harness, COLOR_GROUND_BY);
    click_last_named(&mut harness, "To food");
    let shades_lost = "The ground will no longer be shaded by To food and To home.";
    ask_and_cancel(&mut harness, &mut seen, CHANGE_COLORS_HEADING, shades_lost, &starter);
    click(&mut harness, COLOR_GROUND_BY);
    click_last_named(&mut harness, "To food");
    let shaded = |harness: &Harness| {
        let rule = sheet(harness).document().colors.get("ground").cloned();
        rule.is_some_and(|rule| rule.rule == ColorRuleKind::Shaded && rule.shades.is_empty())
    };
    act_and_undo(&mut harness, CHANGE_COLORS_HEADING, CHANGE, shaded, &starter);

    // The ants' colours hold no shade.
    click(&mut harness, COLOR_AGENTS_BY);
    click_last_named(&mut harness, "Heading");
    assert!(!is_open(&mut harness, CHANGE_COLORS_HEADING));
    let by_heading = sheet(&harness).document().colors.get("ant").map(|rule| rule.rule);
    assert_eq!(by_heading, Some(ColorRuleKind::ByHeading));
    assert_no_forbidden_word(&seen);

    // A start by chance gives way to one state at once: its rows belong to the start the edit replaces.
    open_starter(&mut harness, "sir.toml");
    choose(&mut harness, "Health starts as", "All Susceptible");
    assert!(!is_open(&mut harness, REPLACE_START_HEADING));
    let start = sheet(&harness).document().cells.iter().flatten().next().cloned();
    assert_eq!(
        start.and_then(|health| health.starts_as).map(|start| start.rule),
        Some(StartRule::Value)
    );
}

/// Clicks the last widget named exactly `name`, the item of an open select list under the cards' chips of that name.
fn click_last_named(harness: &mut Harness, name: &str) {
    crate::tests::panes::click_last(harness, name);
    harness.settle();
}

/// Deleting an event that a line records and a measure counts, removing a measure that no control of the Sheet pane
/// makes again, and deleting a button or changing it to start again, which takes the lines of its card, ask first. A
/// count of a state, a count of an event and a button with no line go at once.
#[test]
fn deleting_an_event_a_measure_or_a_button_asks_when_it_takes_more() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "ants.toml");
    let starter = text(&harness);
    let measures = |harness: &Harness| -> Vec<String> {
        let document = sheet(harness).document();
        document.measures.iter().map(|measure| measure.name.clone()).collect()
    };

    click(&mut harness, "Delete event Delivery");
    let delivery_lost = "Line 15 will lose Delivery. The measure Deliveries will be removed.";
    ask_and_cancel(&mut harness, &mut seen, DELETE_EVENT_HEADING, delivery_lost, &starter);
    click(&mut harness, "Delete event Delivery");
    act_and_undo(
        &mut harness,
        DELETE_EVENT_HEADING,
        DELETE,
        |harness| sheet(harness).document().events.is_empty(),
        &starter,
    );

    click(&mut harness, "Remove measure Total trail");
    let question = "Total trail cannot be added again in the Sheet pane.";
    ask_and_cancel(&mut harness, &mut seen, REMOVE_MEASURE_HEADING, question, &starter);
    click(&mut harness, "Remove measure Total trail");
    act_and_undo(
        &mut harness,
        REMOVE_MEASURE_HEADING,
        REMOVE,
        |harness| !measures(harness).contains(&"Total trail".to_owned()),
        &starter,
    );
    // The Plot box of Carrying food makes the count again.
    click(&mut harness, "Remove measure Carrying food");
    assert!(!is_open(&mut harness, REMOVE_MEASURE_HEADING));
    assert!(!measures(&harness).contains(&"Carrying food".to_owned()));
    // The measure form makes a count of Delivery again.
    click(&mut harness, "Remove measure Deliveries");
    assert!(!is_open(&mut harness, REMOVE_MEASURE_HEADING));
    assert!(!measures(&harness).contains(&"Deliveries".to_owned()));

    open_starter(&mut harness, "sir.toml");
    let starter = text(&harness);
    let buttons = |harness: &Harness| sheet(harness).document().buttons.len();
    click(&mut harness, "Delete button New outbreak");
    let outbreak_lost = "New outbreak line 1 will be deleted.";
    ask_and_cancel(&mut harness, &mut seen, DELETE_BUTTON_HEADING, outbreak_lost, &starter);
    click(&mut harness, "Delete button New outbreak");
    act_and_undo(
        &mut harness,
        DELETE_BUTTON_HEADING,
        DELETE,
        |harness| buttons(harness) == 0,
        &starter,
    );

    click(&mut harness, "Edit button New outbreak");
    click(&mut harness, START_AGAIN);
    click(&mut harness, DONE);
    ask_and_cancel(&mut harness, &mut seen, CHANGE_BUTTON_HEADING, outbreak_lost, &starter);
    assert!(shows(&mut harness, DONE), "the button form stays open");
    click(&mut harness, DONE);
    let starts_again = |harness: &Harness| {
        let document = sheet(harness).document();
        let steps = document
            .buttons
            .first()
            .map(|button| button.does.clone())
            .unwrap_or_default();
        steps.iter().any(|step| step.rule == ButtonStepRule::StartAgain) && document.line("l4").is_none()
    };
    act_and_undo(&mut harness, CHANGE_BUTTON_HEADING, CHANGE, starts_again, &starter);

    // A button that runs once with no line goes at once.
    apply_sheet_edit(
        sheet_mut(&mut harness),
        SheetEdit::AddButton {
            name: "Nothing".to_owned(),
            rule: ButtonStepRule::RunOnce,
            thing: "cells".to_owned(),
        },
    );
    click(&mut harness, "Delete button Nothing");
    assert!(!is_open(&mut harness, DELETE_BUTTON_HEADING));
    assert_eq!(buttons(&harness), 1);
    assert_no_forbidden_word(&seen);
}
