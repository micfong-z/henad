//! Tests of the Sheet pane in a world of agents: the random walk's sheet built from a blank model of agents by
//! accessible names, the number and the name of the agents, the kinds and starts of agents' properties, the ground with
//! its marks, fading and blocked states, events, colours and buttons, and the words rule over every new section.

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use henad_compute::entry::ModelEntry;
use henad_core::params::ParamValue;
use henad_graph::sheet::SheetFile;
use henad_graph::sheet::document::{
    BlockedState, ButtonStep, ButtonStepRule, ColorRuleKind, MadeOf, Marks, ParameterType, PropertyType, SheetLiteral,
    SheetValue, Slot, StartRule,
};
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::sentence::{
    ADD_BUTTON, ADD_EVENT, ADD_GROUND_PROPERTY_NAME, ADD_MEASURE, ADD_PARAMETER, AGENT_COUNT_NAME, AGENT_MAX_NAME,
    AGENT_NAME_FIELD, AGENT_NAME_MISSING, AGENTS_IN_OPEN_SPACE, ANYWHERE_AT_RANDOM, ARROW_KIND, BUTTON_NAME_FIELD,
    BY_HEADING_NOTE, CANCEL, COLOR_AGENTS_BY, COLOR_GROUND_BY, COUNT_AGENTS_MEASURE, CREATE_PARAMETER, DONE,
    EVENT_NAME_FIELD, EVENT_NAME_MISSING, EVENTS_EMPTY, GROUND_EMPTY, HEADING_START, KEPT_OUT_OF_RANGE, NEW_PARAMETER,
    NUMBER_KIND, PAINTED, SHARE_LIMITS_OUTSIDE, START_AGAIN_FOR, STATES_KIND, YES_NO_KIND, add_property_name,
    blocked_state_name, count_agents_measure, event_name_taken, integer_up_to, marks_name, run_once, value_field_name,
};
use henad_graph::sheet::sentence::{MARKS_ADD_UP, MARKS_LARGEST, MARKS_NONE};

use crate::state::default_values;
use crate::tests::panes::{
    Seen, add_average_measure, add_property, add_states_property, agent_sheet, apply_sheet_edit, assert_control_holds,
    assert_no_forbidden_word, center_of, choose, click, click_control, enter_into, field_value, named,
    new_blank_agents, sheet, sheet_mut, shows_or_notice, starter_document, text, type_into,
};
use crate::tests::support::{Harness, Node, align_with_starter, set_of, without_card_lines};
use crate::ui::editor::EditorModel;
use crate::ui::editor::sheet::SheetEditor;
use crate::ui::editor::sheet::new_model::create_blank_agent_model;
use crate::ui::params::notice_content;
use crate::ui::sweep::session::{SessionExecution, SweepSession};
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::spec::{BlockSpec, SweepSpec};

/// Returns a harness whose Editor holds a blank model of agents on a tall screen, or `None` to skip on a machine
/// without a device.
fn blank_agents() -> Option<Harness> {
    let mut harness = Harness::tall()?;
    create_blank_agent_model(&mut harness.app, "");
    harness.settle();
    Some(harness)
}

/// Opens the New model dialog and creates a model from the starter named `choice` under its own name.
fn new_from_starter(harness: &mut Harness, choice: &str) {
    click(harness, "New model");
    click(harness, choice);
    click(harness, "Create");
    harness.settle();
}

/// Adds a parameter named `name` keeping `default` in `min` to `max` through "+ Add parameter".
fn add_parameter(harness: &mut Harness, name: &str, default: &str, min: &str, max: &str) {
    click(harness, ADD_PARAMETER);
    type_into(harness, "Parameter name", name);
    type_into(harness, "Max", max);
    type_into(harness, "Min", min);
    type_into(harness, "Default value", default);
    click(harness, CREATE_PARAMETER);
    harness.settle();
}

/// Returns the names of the radio buttons a settled frame shows, in reading order.
fn radio_names(harness: &mut Harness) -> Vec<String> {
    let nodes = harness.settle();
    let mut radios: Vec<&Node> = nodes.iter().filter(|node| node.role == Role::RadioButton).collect();
    radios.sort_by(|a, b| {
        let (a, b) = (a.bounds.expect("bounds").min, b.bounds.expect("bounds").min);
        (a.y, a.x).partial_cmp(&(b.y, b.x)).expect("finite bounds")
    });
    radios.iter().map(|node| node.label.clone()).collect()
}

/// Opens the select named `select` and returns, sorted and once each, the texts it shows that start a Position on the
/// ground. The select stays open.
fn on_ground_offers(harness: &mut Harness, select: &str) -> Vec<String> {
    click(harness, select);
    let mut offers: Vec<String> = harness
        .accessible_texts()
        .into_iter()
        .filter(|shown| shown.starts_with("On a cell whose "))
        .collect();
    offers.sort();
    offers.dedup();
    offers
}

#[test]
fn a_blank_agent_model_builds_the_random_walk_sheet_by_accessible_names() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    new_blank_agents(&mut harness, "Random walk");
    assert_eq!(sheet(&harness).document().world.made_of, MadeOf::Agents);
    assert_eq!(field_value(&mut harness, AGENT_NAME_FIELD), "Agent");
    assert!(
        shows_or_notice(&mut harness, ANYWHERE_AT_RANDOM),
        "Position starts anywhere at random"
    );
    assert!(shows_or_notice(&mut harness, HEADING_START));
    assert!(shows_or_notice(&mut harness, GROUND_EMPTY));
    assert!(shows_or_notice(&mut harness, EVENTS_EMPTY));

    let starter = starter_document("random_walk.toml");
    agent_sheet(&mut harness, &starter, "Walker", "10000");
    assert!(shows_or_notice(&mut harness, BY_HEADING_NOTE));
    add_parameter(&mut harness, "Steps per tick", "4", "0", "16");
    add_parameter(&mut harness, "Step size", "2", "0.1", "20");
    add_average_measure(&mut harness, "Position");

    // Only a counting blank on the card makes Steps per tick an integer, and this rebuild adds no card line.
    let mut expected = without_card_lines(&starter);
    for parameter in &mut expected.parameters {
        if parameter.id == "steps_per_tick" {
            parameter.parameter_type = ParameterType::Number;
        }
    }
    let aligned = align_with_starter(sheet(&harness).document(), &expected);
    assert_eq!(
        aligned.to_text().expect("the rebuild writes"),
        expected.to_text().expect("the starter writes"),
        "the rebuild equals the starter without its card lines"
    );
}

#[test]
fn the_agents_take_a_name_and_an_integer_up_to_the_most_the_world_holds() {
    let Some(mut harness) = blank_agents() else {
        return;
    };
    let kind = |harness: &Harness| sheet(harness).document().agents[0].clone();
    for typed in ["0", "1.5", "abc", "1000001"] {
        enter_into(&mut harness, AGENT_COUNT_NAME, typed);
        assert!(
            shows_or_notice(&mut harness, &integer_up_to(1_000_000)),
            "{typed} is refused"
        );
        assert_eq!(kind(&harness).count, 1000);
    }
    enter_into(&mut harness, AGENT_COUNT_NAME, "2000");
    assert_eq!(kind(&harness).count, 2000);
    // A smaller world holds fewer agents at the start too.
    enter_into(&mut harness, AGENT_MAX_NAME, "500");
    assert_eq!((kind(&harness).count, kind(&harness).max_count), (500, 500));
    enter_into(&mut harness, AGENT_MAX_NAME, "20000000");
    assert!(shows_or_notice(
        &mut harness,
        &integer_up_to(henad_graph::limits::MAX_AGENTS)
    ));

    // The kind's id follows its name, and the name of its "+ Add property" follows too.
    enter_into(&mut harness, AGENT_NAME_FIELD, "Bird");
    assert_eq!(
        (kind(&harness).id.as_str(), kind(&harness).name.as_str()),
        ("bird", "Bird")
    );
    center_of(&mut harness, &add_property_name("Bird"));
    enter_into(&mut harness, AGENT_NAME_FIELD, " ");
    assert!(shows_or_notice(&mut harness, AGENT_NAME_MISSING));
    assert_eq!(kind(&harness).name, "Bird");
}

#[test]
fn agents_take_states_numbers_and_yes_or_no_and_no_start_on_an_unpainted_ground() {
    let Some(mut harness) = blank_agents() else {
        return;
    };
    let add = add_property_name("Agent");
    let property = |harness: &Harness, id: &str| {
        sheet(harness).document().agents[0]
            .properties
            .iter()
            .find(|property| property.id == id)
            .cloned()
            .expect("the property")
    };

    // A Position and a Heading are offered only while the agents lack one. An Arrow is always offered.
    click(&mut harness, &add);
    assert_eq!(
        radio_names(&mut harness),
        [STATES_KIND, NUMBER_KIND, "Integer", YES_NO_KIND, ARROW_KIND]
    );
    click(&mut harness, CANCEL);

    add_property(&mut harness, &add, "Hungry", YES_NO_KIND);
    assert_eq!(
        property(&harness, "hungry").starts_as.map(|start| start.value),
        Some(Slot::Filled(SheetValue::Literal(SheetLiteral::Bool(false))))
    );
    choose(&mut harness, "Hungry starts as", "All Yes");
    assert_eq!(
        property(&harness, "hungry").starts_as.map(|start| start.value),
        Some(Slot::Filled(SheetValue::Literal(SheetLiteral::Bool(true))))
    );
    add_property(&mut harness, &add, "Energy", NUMBER_KIND);
    enter_into(&mut harness, "Energy starts as: number", "5");
    assert!(shows_or_notice(&mut harness, "Every agent starts at"));

    // No control paints the ground, so a Position cannot start on a state of a ground made here.
    add_states_property(&mut harness, ADD_GROUND_PROPERTY_NAME, "Ground", &["Open", "Nest"]);
    click(&mut harness, DONE);
    harness.settle();
    let ground = sheet(&harness).document().ground.clone().expect("a ground");
    assert_eq!(ground.properties[0].property_type, PropertyType::States);
    assert_eq!(
        on_ground_offers(&mut harness, "Position starts as"),
        Vec::<String>::new()
    );
    click_control(&mut harness, ANYWHERE_AT_RANDOM);
    assert_eq!(
        property(&harness, "position").starts_as.map(|start| start.rule),
        Some(StartRule::Anywhere)
    );

    // A count of agents in a state reads in the agents' words.
    add_states_property(&mut harness, &add, "Mood", &["Calm", "Angry"]);
    click(&mut harness, DONE);
    harness.settle();
    click(&mut harness, "Plot Angry");
    let measure = sheet(&harness).document().measures[0].clone();
    assert_eq!(measure.over.as_deref(), Some("agent"));
    assert!(shows_or_notice(&mut harness, &count_agents_measure("Mood", "Angry")));
    click(&mut harness, ADD_MEASURE);
    assert!(shows_or_notice(&mut harness, COUNT_AGENTS_MEASURE));
    click(&mut harness, CANCEL);

    // A button runs once over every agent, and starts the ground's properties again too.
    click(&mut harness, ADD_BUTTON);
    type_into(&mut harness, BUTTON_NAME_FIELD, "Scatter");
    click(&mut harness, &run_once("agent"));
    click(&mut harness, DONE);
    harness.settle();
    click(&mut harness, ADD_BUTTON);
    type_into(&mut harness, BUTTON_NAME_FIELD, "Repaint");
    choose(&mut harness, START_AGAIN_FOR, "Ground");
    click(&mut harness, DONE);
    harness.settle();
    let buttons = sheet(&harness).document().buttons.clone();
    assert_eq!(buttons[0].does[0].rule, ButtonStepRule::RunOnce);
    let card = buttons[0].does[0].card.clone().expect("a card");
    assert!(
        sheet(&harness)
            .document()
            .cards
            .iter()
            .any(|known| known.id == card && known.every == "agent")
    );
    assert_eq!(buttons[1].does[0].properties, ["ground"]);
}

#[test]
fn a_ground_property_adds_up_its_marks_fades_and_blocks_agents() {
    let Some(mut harness) = blank_agents() else {
        return;
    };
    assert!(shows_or_notice(&mut harness, GROUND_EMPTY));
    // The ground holds only numbers and states.
    click(&mut harness, ADD_GROUND_PROPERTY_NAME);
    assert_eq!(radio_names(&mut harness), [STATES_KIND, NUMBER_KIND]);
    click(&mut harness, CANCEL);
    add_property(&mut harness, ADD_GROUND_PROPERTY_NAME, "Trail", NUMBER_KIND);
    let trail = |harness: &Harness| sheet(harness).document().ground.clone().expect("a ground").properties[0].clone();
    assert_eq!(trail(&harness).marks, None);
    choose(&mut harness, &marks_name("Trail"), MARKS_LARGEST);
    assert_eq!(trail(&harness).marks, Some(Marks::Largest));
    assert_eq!(
        trail(&harness).kept_per_tick,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(1.0))),
        "a property that takes marks keeps all of it each tick to start with"
    );
    choose(&mut harness, &marks_name("Trail"), MARKS_ADD_UP);
    assert_eq!(trail(&harness).marks, Some(Marks::AddUp));
    assert!(text(&harness).contains("marks = \"add_up\""));
    let lowered = SheetFile::parse(&text(&harness)).expect("the sheet lowers");
    assert!(
        lowered.lowered().to_toml().contains("combine = \"sum\""),
        "add up lowers to a sum"
    );

    // The share kept each tick is from 0 to 1, and no marks rule takes it away.
    for typed in ["2", " "] {
        enter_into(&mut harness, "Trail, share kept each tick: number", typed);
        assert!(shows_or_notice(&mut harness, KEPT_OUT_OF_RANGE), "{typed:?} is refused");
    }
    enter_into(&mut harness, "Trail, share kept each tick: number", "0.99");
    assert_eq!(
        trail(&harness).kept_per_tick,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.99)))
    );
    assert!(text(&harness).contains("kept_per_tick = 0.99"));
    choose(&mut harness, &marks_name("Trail"), MARKS_NONE);
    assert_eq!(
        (trail(&harness).marks, trail(&harness).kept_per_tick),
        (None, Slot::Blank)
    );

    // A box keeps agents out of the cells of a state.
    add_states_property(&mut harness, ADD_GROUND_PROPERTY_NAME, "Terrain", &["Open", "Rock"]);
    click(&mut harness, DONE);
    harness.settle();
    click(&mut harness, &blocked_state_name("Rock"));
    let blocked = |harness: &Harness| sheet(harness).document().ground.clone().expect("a ground").blocked;
    assert_eq!(blocked(&harness), [BlockedState::new("terrain", "rock")]);
    click(&mut harness, &blocked_state_name("Rock"));
    assert!(blocked(&harness).is_empty());

    // The ground is coloured on its own, and goes with its last property.
    choose(&mut harness, COLOR_GROUND_BY, "Trail");
    assert_eq!(
        sheet(&harness).document().colors.get("ground").map(|rule| rule.rule),
        Some(ColorRuleKind::Shaded)
    );
    for name in ["Trail", "Terrain"] {
        click(&mut harness, &format!("Edit property {name}"));
        click(&mut harness, "Delete property");
        harness.settle();
    }
    assert_eq!(sheet(&harness).document().ground, None);
    assert!(shows_or_notice(&mut harness, GROUND_EMPTY));
}

/// Clicks the Number box of the open picker titled `dialog`, the field named by its hint, types `text` and presses
/// Enter.
fn enter_in_picker(harness: &mut Harness, dialog: &str, text: &str) {
    let nodes = harness.settle();
    assert!(
        nodes
            .iter()
            .any(|node| node.role == Role::Dialog && node.label == dialog),
        "{dialog} is open"
    );
    let field = nodes
        .iter()
        .find(|node| node.role == Role::TextInput && node.label.ends_with(", then press Enter."))
        .and_then(|node| node.bounds)
        .expect("the Number box");
    harness.click_at(field.center());
    harness.replace_text(text);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
}

/// The share kept each tick refuses a number outside 0 to 1 typed in the picker of its parameter chip, and a new
/// parameter made there starts in 0 to 1 and refuses a range outside it.
#[test]
fn a_share_kept_through_its_parameter_chip_stays_from_0_to_1() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    harness.open_sheet(&starter_document("ants.toml").to_text().expect("the starter writes"));
    let before = sheet(&harness).document().clone();
    let chip = named(&mut harness, "Trail kept per tick")
        .into_iter()
        .find(|node| node.role == Role::Button)
        .and_then(|node| node.bounds)
        .expect("the chip of the share kept");
    harness.click_at(chip.center());
    enter_in_picker(&mut harness, "Choose a number", "5");
    assert_eq!(sheet(&harness).document(), &before, "5 is no share");
    assert!(shows_or_notice(&mut harness, KEPT_OUT_OF_RANGE));
    enter_in_picker(&mut harness, "Choose a number", "0.5");
    let kept = |harness: &Harness| {
        sheet(harness).document().ground.clone().expect("a ground").properties[1]
            .kept_per_tick
            .clone()
    };
    assert_eq!(
        kept(&harness),
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.5)))
    );

    // The field holds 0.5 now. Its "Use a parameter" offers New parameter, which starts from it in 0 to 1.
    click(&mut harness, "To food, share kept each tick: use a parameter");
    click(&mut harness, NEW_PARAMETER);
    assert_eq!(field_value(&mut harness, "Default value"), "0.5");
    assert_eq!(field_value(&mut harness, "Max"), "1");
    type_into(&mut harness, "Parameter name", "Fade");
    type_into(&mut harness, "Max", "2");
    click(&mut harness, CREATE_PARAMETER);
    assert!(shows_or_notice(&mut harness, SHARE_LIMITS_OUTSIDE));
}

/// A value typed for the parameter of the share kept each tick widens the parameter's range past 1, and the sheet
/// names the error.
#[test]
fn a_share_kept_parameter_widened_past_1_is_an_error() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    harness.open_sheet(&starter_document("ants.toml").to_text().expect("the starter writes"));
    assert!(!sheet(&harness).has_errors());
    enter_into(&mut harness, &value_field_name("Trail kept per tick"), "5");
    let parameters = &sheet(&harness).document().parameters;
    let kept = parameters
        .iter()
        .find(|parameter| parameter.name == "Trail kept per tick")
        .expect("the parameter");
    assert_eq!(kept.max, 5.0, "the typed value widens the range");
    assert!(sheet(&harness).has_errors());
    assert!(shows_or_notice(&mut harness, SHARE_LIMITS_OUTSIDE));
}

#[test]
fn events_are_added_renamed_and_deleted_on_the_sheet() {
    let Some(mut harness) = blank_agents() else {
        return;
    };
    let events = |harness: &Harness| sheet(harness).document().events.clone();
    click(&mut harness, ADD_EVENT);
    click(&mut harness, DONE);
    assert!(shows_or_notice(&mut harness, EVENT_NAME_MISSING));
    type_into(&mut harness, EVENT_NAME_FIELD, "Delivery");
    click(&mut harness, DONE);
    harness.settle();
    assert_eq!(events(&harness)[0].id, "delivery");
    click(&mut harness, ADD_EVENT);
    type_into(&mut harness, EVENT_NAME_FIELD, "delivery");
    harness.press(Key::Enter, Modifiers::NONE);
    assert!(shows_or_notice(&mut harness, &event_name_taken("delivery")));
    click(&mut harness, CANCEL);
    enter_into(&mut harness, "Name of event Delivery", "Arrival");
    assert_eq!(
        (events(&harness)[0].id.as_str(), events(&harness)[0].name.as_str()),
        ("delivery", "Arrival")
    );
    click(&mut harness, "Delete event Arrival");
    harness.settle();
    assert!(events(&harness).is_empty());
    assert!(shows_or_notice(&mut harness, EVENTS_EMPTY));
}

/// The button form keeps the steps that set a property everywhere. It shows them, and Done neither drops nor rewrites
/// them: on the ants' Reset colony as the starter holds it, on a button that only sets, and on a change of kind.
#[test]
fn the_button_form_keeps_the_steps_that_set_a_property_everywhere() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    new_from_starter(&mut harness, "Ant foraging");
    let steps = |harness: &Harness| {
        sheet(harness)
            .document()
            .buttons
            .iter()
            .find(|button| button.name == "Reset colony")
            .expect("the starter's Reset colony")
            .does
            .clone()
    };
    let sets: Vec<ButtonStep> = steps(&harness)
        .into_iter()
        .filter(|step| step.rule == ButtonStepRule::SetEverywhere)
        .collect();
    assert_eq!(sets.len(), 2, "{sets:?}");
    let starter = text(&harness);
    click(&mut harness, "Edit button Reset colony");
    assert!(shows_or_notice(&mut harness, "Set To food and To home to 0 everywhere"));
    click(&mut harness, DONE);
    harness.settle();
    assert_eq!(text(&harness), starter);

    let only_sets = sets.clone();
    apply_sheet_edit(
        sheet_mut(&mut harness),
        SheetEdit::SetButtonSteps {
            button: "reset_colony".to_owned(),
            does: only_sets,
        },
    );
    let only_sets = text(&harness);
    click(&mut harness, "Edit button Reset colony");
    assert!(shows_or_notice(&mut harness, "Set To food and To home to 0 everywhere"));
    click(&mut harness, DONE);
    harness.settle();
    assert_eq!(text(&harness), only_sets, "a button that only sets keeps its steps");

    click(&mut harness, "Edit button Reset colony");
    click(&mut harness, &run_once("ant"));
    click(&mut harness, DONE);
    harness.settle();
    let after = steps(&harness);
    assert_eq!(
        after.first().map(|step| step.rule),
        Some(ButtonStepRule::RunOnce),
        "{after:?}"
    );
    assert_eq!(
        after.get(1..),
        Some(sets.as_slice()),
        "the set steps follow the new step"
    );
}

#[test]
fn the_ant_starter_shows_its_ground_and_starts_ants_on_single_painted_cells() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    new_from_starter(&mut harness, "Ant foraging");
    assert_eq!(field_value(&mut harness, AGENT_NAME_FIELD), "Ant");
    assert_eq!(field_value(&mut harness, AGENT_COUNT_NAME), "2000");
    let ground = sheet(&harness).document().ground.clone().expect("a ground");
    assert_eq!(ground.properties[1].marks, Some(Marks::Largest));
    assert!(shows_or_notice(&mut harness, MARKS_LARGEST));
    let nodes = harness.settle();
    assert!(
        nodes
            .iter()
            .any(|node| node.role == Role::Button && node.label == "Trail kept per tick"),
        "the share kept each tick shows its parameter as a chip"
    );
    assert!(shows_or_notice(&mut harness, "Open, with 4 shapes painted on it"));
    assert!(shows_or_notice(&mut harness, "On a cell whose Ground is Nest"));

    // The painted start shows in its select, and choosing it again changes nothing.
    let before = text(&harness);
    click(&mut harness, "Ground starts as");
    click_control(&mut harness, PAINTED);
    assert_eq!(text(&harness), before);

    // The ants start only on a state painted on single cells, and the sheet still lowers after the choice.
    assert_eq!(
        on_ground_offers(&mut harness, "Position starts as"),
        ["On a cell whose Ground is Food", "On a cell whose Ground is Nest"]
    );
    click_control(&mut harness, "On a cell whose Ground is Food");
    let position = sheet(&harness).document().agents[0]
        .properties
        .iter()
        .find(|property| property.id == "position")
        .and_then(|property| property.starts_as.clone())
        .expect("a start");
    assert_eq!(
        (position.rule, position.property.as_deref(), position.state.as_deref()),
        (StartRule::OnGround, Some("ground"), Some("food"))
    );
    SheetFile::parse(&text(&harness)).expect("a start on the food lowers");
    assert!(shows_or_notice(
        &mut harness,
        "Start again: every property, Set To food and To home to 0 everywhere"
    ));
    let nodes = harness.settle();
    let rock = nodes
        .iter()
        .find(|node| node.label == blocked_state_name("Obstacle"))
        .expect("the Obstacle box");
    assert_eq!(rock.role, Role::CheckBox);
}

#[test]
fn the_agent_sections_show_no_forbidden_word_in_any_state() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    click(&mut harness, "New model");
    click(&mut harness, AGENTS_IN_OPEN_SPACE);
    seen.look(&mut harness);
    click(&mut harness, "Create");
    seen.look(&mut harness);
    let add = add_property_name("Agent");
    click(&mut harness, &add);
    seen.look(&mut harness);
    click(&mut harness, YES_NO_KIND);
    seen.look(&mut harness);
    click(&mut harness, NUMBER_KIND);
    seen.look(&mut harness);
    click(&mut harness, CANCEL);
    add_property(&mut harness, &add, "Hungry", YES_NO_KIND);
    seen.look(&mut harness);
    click(&mut harness, "Hungry starts as");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, "Position starts as");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    enter_into(&mut harness, AGENT_COUNT_NAME, "0");
    seen.look(&mut harness);
    enter_into(&mut harness, AGENT_NAME_FIELD, " ");
    seen.look(&mut harness);
    click(&mut harness, ADD_GROUND_PROPERTY_NAME);
    seen.look(&mut harness);
    click(&mut harness, NUMBER_KIND);
    type_into(&mut harness, "Property name", "Trail");
    click(&mut harness, DONE);
    seen.look(&mut harness);
    click(&mut harness, &marks_name("Trail"));
    seen.look(&mut harness);
    click(&mut harness, MARKS_ADD_UP);
    seen.look(&mut harness);
    enter_into(&mut harness, "Trail, share kept each tick: number", "abc");
    seen.look(&mut harness);
    click(&mut harness, "Trail, share kept each tick: use a parameter");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    add_states_property(&mut harness, ADD_GROUND_PROPERTY_NAME, "Terrain", &["Open", "Rock"]);
    click(&mut harness, DONE);
    seen.look(&mut harness);
    click(&mut harness, ADD_EVENT);
    seen.look(&mut harness);
    click(&mut harness, DONE);
    seen.look(&mut harness);
    click(&mut harness, CANCEL);
    click(&mut harness, COLOR_AGENTS_BY);
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, COLOR_GROUND_BY);
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, ADD_MEASURE);
    seen.look(&mut harness);
    click(&mut harness, CANCEL);
    click(&mut harness, ADD_BUTTON);
    seen.look(&mut harness);
    click(&mut harness, CANCEL);
    for help in ["Help: Agents", "Help: Ground", "Help: Events"] {
        click(&mut harness, help);
        seen.look(&mut harness);
        harness.press(Key::Escape, Modifiers::NONE);
    }
    for starter in ["boids.toml", "bounded_confidence.toml", "ants.toml"] {
        let text = starter_document(starter).to_text().expect("the starter writes");
        harness.open_sheet(&text);
        seen.look(&mut harness);
    }
    assert!(seen.looks() >= 25, "only {} states were drawn", seen.looks());
    assert_no_forbidden_word(&seen);

    // Each control with words of its own holds them in its name.
    harness.open_sheet(&starter_document("ants.toml").to_text().expect("the starter writes"));
    let nodes = harness.settle();
    for visible in ["+ Add property", ADD_EVENT, "Obstacle", "To food", "Cargo"] {
        assert_control_holds(&nodes, visible);
    }
    click(&mut harness, &add_property_name("Ant"));
    let nodes = harness.settle();
    for visible in [DONE, CANCEL, STATES_KIND, NUMBER_KIND, YES_NO_KIND] {
        assert_control_holds(&nodes, visible);
    }
}

/// Puts the boids sheet in the harness's Editor, and returns the entry its program compiles to.
fn boids_sheet_entry(harness: &mut Harness) -> ModelEntry {
    harness.app.editor.model = EditorModel::Sheet(Box::new(SheetEditor::new(starter_document("boids.toml"))));
    sheet_mut(harness).program().expect("the boids sheet compiles").entry()
}

/// Work past the budget on the boids sheet in the Editor names the lines of its looks in the sheet's words, where the
/// entry's own reasons name a pass of the lowered graph.
#[test]
fn work_past_the_budget_on_a_sheet_names_its_lines_and_no_pass() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let entry = boids_sheet_entry(&mut harness);
    let mut values = default_values(&entry);
    for (id, value) in [
        ("num_agents", ParamValue::U32(70_000)),
        ("world_width", ParamValue::F32(200.0)),
        ("world_height", ParamValue::F32(200.0)),
        ("visual_range", ParamValue::F32(200.0)),
    ] {
        let index = entry
            .param_descriptors()
            .iter()
            .position(|descriptor| descriptor.id() == id)
            .expect("a boids parameter");
        values[index] = value;
    }
    let raw = entry.shortfalls(&values, &harness.app.runtime.granted);
    assert!(raw.iter().any(|reason| reason.contains("pass")), "{raw:?}");

    let shortfalls = harness.app.entry_shortfalls(&entry, &values);
    assert!(
        shortfalls
            .iter()
            .any(|reason| reason.starts_with("Lines 1 and 2, Flockmates and Too close: looks at too many birds")),
        "{shortfalls:?}"
    );
    assert!(
        shortfalls.iter().all(|reason| !reason.contains("pass")),
        "{shortfalls:?}"
    );

    // The Parameters tab's notice ends each reason with one full stop.
    let notice = notice_content(&harness.app, 0, &shortfalls).expect("a notice");
    assert!(
        notice
            .detail
            .ends_with("Use a shorter distance or fewer birds. Reduce the size parameters to build."),
        "{}",
        notice.detail
    );
    assert!(!notice.detail.contains(".."), "{}", notice.detail);
}

/// A sweep of the Editor's boids sheet with a config past the work budget is refused at Start in the sheet's lines, as
/// Build refuses a single run, and names no pass.
#[test]
fn a_sweep_past_the_work_budget_on_a_sheet_names_its_lines_and_no_pass() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let entry = boids_sheet_entry(&mut harness);
    harness.app.install_models(&set_of(entry.clone()));
    let mut spec = SweepSpec::new(entry.id());
    spec.fixed = [("world_width", "200"), ("world_height", "200"), ("visual_range", "200")]
        .map(|(id, value)| (id.to_owned(), value.to_owned()))
        .to_vec();
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::param(
            "num_agents",
            LevelSpec::Values(vec!["100".to_owned(), "70000".to_owned()]),
        )],
        design_seed: None,
    }];
    let Err(refusal) = SweepSession::start(&mut harness.app, spec, SessionExecution::default(), None) else {
        panic!("the sweep started");
    };
    assert!(refusal.contains("1 config does not fit"), "{refusal}");
    assert!(
        refusal.contains("config 1: Lines 1 and 2, Flockmates and Too close: looks at too many birds at 70,000 birds."),
        "{refusal}"
    );
    for word in ["pass", "units of work"] {
        assert!(!refusal.contains(word), "{refusal}");
    }
}
