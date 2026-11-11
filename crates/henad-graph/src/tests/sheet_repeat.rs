//! The order of changes inside a Repeat: among competing changes of one property, the line whose condition first held
//! at the earliest time round wins, and the higher line wins a tie. Shares of agents on the host and the device
//! against their closed forms, a Become from a state that never overrules an agent in another state, and display
//! order whatever the line ids and against a change outside the Repeat.

use henad_core::params::ParamValue;

use super::sheet_support::{
    ROLL, accumulators, agent_program_of, edited, host_of, lowered, pass_nodes, read, with_sets,
};
use super::support::{built, context, device_for, params_of};
use crate::host::{HostValue, HostWorld, RunContext};
use crate::node::Node;
use crate::sheet::SheetFile;
use crate::sheet::relations::CardRelations;

/// Text of the test sheet whose Repeat of 4 becomes A with chance 0.1, and otherwise B with chance 0.5.
const CHANCES: &str = include_str!("sheets/repeat_chances.toml");

/// Agents the host runs for one share.
const HOST_AGENTS: u32 = 20_000;

/// Seed of every run.
const SEED: u64 = 7;

/// Share of the chances sheet that becomes A under the first-time rule: `0.1 (1 - 0.45^4) / 0.55`.
const CHANCE_A: f64 = 0.174_362_5;

/// Share of the chances sheet that becomes B under the first-time rule: `0.45 (1 - 0.45^4) / 0.55`.
const CHANCE_B: f64 = 0.784_631_25;

/// Share of the chances sheet that no line changes: `0.45^4`.
const CHANCE_NONE: f64 = 0.041_006_25;

/// Returns `text` with the ids of lines `l3` and `l4` swapped.
fn swapped_ids(text: &str) -> String {
    text.replace("l3", "l_swap").replace("l4", "l3").replace("l_swap", "l4")
}

/// Returns the shares of `agents` agents of the sheet `text` whose property `property` holds each of `values` after
/// one tick on the host.
fn host_shares(text: &str, agents: u32, property: &str, values: &[HostValue]) -> Vec<f64> {
    let file = lowered(text);
    let evaluator = host_of(&file);
    let world = HostWorld::Agents {
        count: agents,
        width: 1000.0,
        height: 1000.0,
    };
    let mut lanes = evaluator.init(world, &[], Some(SEED)).expect("the init runs");
    evaluator
        .run_pass(
            "tick_walker_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(SEED), 0),
            &mut 0,
        )
        .expect("a tick runs");
    let column = lanes.column(property).expect("the property's lane");
    values
        .iter()
        .map(|value| {
            let count = (0..agents as usize)
                .filter(|&index| column.get(index).as_ref() == Some(value))
                .count();
            count as f64 / f64::from(agents)
        })
        .collect()
}

/// Asserts that each of `shares` lies within five standard errors of the matching share of `expected` over `agents`
/// agents.
fn assert_shares(shares: &[f64], expected: &[f64], agents: u32, about: &str) {
    for (share, wanted) in shares.iter().zip(expected) {
        let standard_error = (wanted * (1.0 - wanted) / f64::from(agents)).sqrt();
        assert!(
            (share - wanted).abs() <= 5.0 * standard_error,
            "{about}: shares {shares:?}, expected {expected:?}"
        );
    }
}

/// Returns the shares of the agents of `text` in the states S, A and B after one tick on the host.
fn status_shares(text: &str) -> Vec<f64> {
    host_shares(
        text,
        HOST_AGENTS,
        "status",
        &[HostValue::Category(0), HostValue::Category(1), HostValue::Category(2)],
    )
}

/// Returns the names of the accumulators of the Repeat of `file`.
fn repeat_accumulators(file: &SheetFile) -> Vec<String> {
    let region: &Node = file
        .lowered()
        .document()
        .passes
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .find(|node| node.op == "region.repeat")
        .expect("the Repeat's region");
    accumulators(region)
}

/// Returns the chances sheet with a state C, agents that start in `start`, and a Repeat of 4 holding one Become of
/// Status by chance for each of `lines`, in display order.
///
/// Each of `lines` is a line id, the state the line changes from, the state it becomes and its chance.
fn becomes_by_chance(start: &str, lines: &[(&str, &str, &str, f64)]) -> String {
    let (head, _) = CHANCES.split_once("[[card]]").expect("the chances sheet holds a card");
    let head = edited(
        head,
        "{ id = \"b\", name = \"B\", color = \"#E43748\" },",
        "{ id = \"b\", name = \"B\", color = \"#E43748\" },\n  { id = \"c\", name = \"C\", color = \"#3D7DD5\" },",
    );
    let head = edited(&head, "next_line = 4", "next_line = 100");
    let head = edited(
        &head,
        "value = { state = \"s\" }",
        &format!("value = {{ state = \"{start}\" }}"),
    );
    let ids: Vec<String> = lines.iter().map(|(id, ..)| format!("\"{id}\"")).collect();
    let mut text = format!(
        "{head}[[card]]\nid = \"walker_tick\"\nevery = \"walker\"\nruns = \"each_tick\"\nlines = [\"l1\"]\n\n\
         [card.line.l1]\nkind = \"repeat\"\ntimes = 4\nlines = [{}]\n",
        ids.join(", ")
    );
    for (index, (id, from, to, chance)) in lines.iter().enumerate() {
        text.push_str(&format!(
            "\n[card.line.{id}]\nkind = \"become\"\nkey = {}\nproperty = \"status\"\nnew_state = \"{to}\"\n\
             start_state = \"{from}\"\n\n[card.line.{id}.condition]\nkind = \"chance\"\nchance = {chance}\n",
            101 + index
        ));
    }
    text
}

/// Returns the shares of the agents of `text` in the states S, A, B and C after one tick on the host.
fn four_shares(text: &str) -> Vec<f64> {
    host_shares(
        text,
        HOST_AGENTS,
        "status",
        &[
            HostValue::Category(0),
            HostValue::Category(1),
            HostValue::Category(2),
            HostValue::Category(3),
        ],
    )
}

/// The two Becomes that test one roll split the agents evenly, whatever their line ids, since the first time round
/// decides. Under the any-time rule the higher line took 0.9375 of them. Each competing line keeps its first time round
/// in place of an `or` accumulator.
#[test]
fn becomes_that_test_one_roll_inside_a_repeat_split_evenly() {
    for text in [ROLL.to_owned(), swapped_ids(ROLL)] {
        assert_shares(&status_shares(&text), &[0.0, 0.5, 0.5], HOST_AGENTS, &text);
    }
    assert_eq!(repeat_accumulators(&lowered(ROLL)), ["l3_first", "l4_first"]);
}

/// Two Becomes inside a Repeat that test a Random number drawn inside it can each hold at a different time round, so
/// they can both apply. The lower reads "otherwise", and the rows of the transition keep display order whatever the
/// line ids.
#[test]
fn becomes_that_test_a_number_drawn_inside_a_repeat_keep_display_order() {
    for (text, first, second) in [(ROLL.to_owned(), "l3", "l4"), (swapped_ids(ROLL), "l4", "l3")] {
        let file = lowered(&text);
        let transition = pass_nodes(&file, "tick_walker_tick")
            .iter()
            .find(|node| node.op == "cat.transition")
            .expect("the transition of Status");
        let targets: Vec<&str> = transition.config["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .filter_map(|rule| rule.as_table()?.get("to")?.as_str())
            .collect();
        assert_eq!(targets, ["a", "b"], "{first} above {second}");
        let document = read(&text);
        let relations = CardRelations::new(&document, &document.cards[0]);
        assert!(relations.can_both_apply(first, second));
        assert_eq!(relations.otherwise_numbers(second), [3], "{first} above {second}");
    }
}

/// Three Becomes with chances 0.2, 0.3 and 0.5 inside a Repeat of 4 decide a time round with chances 0.2, 0.24 and
/// 0.28, and each takes that chance times `(1 - 0.28^4) / 0.72` of the agents, whatever the order of their line ids.
#[test]
fn three_becomes_inside_a_repeat_follow_the_first_time_rule_in_any_id_order() {
    let rounds = (1.0 - 0.28_f64.powi(4)) / 0.72;
    let expected = [0.28_f64.powi(4), 0.2 * rounds, 0.24 * rounds, 0.28 * rounds];
    for [first, second, third] in [["l2", "l3", "l4"], ["l4", "l2", "l3"], ["l3", "l4", "l2"]] {
        let text = becomes_by_chance(
            "s",
            &[(first, "s", "a", 0.2), (second, "s", "b", 0.3), (third, "s", "c", 0.5)],
        );
        assert_shares(&four_shares(&text), &expected, HOST_AGENTS, &text);
    }
}

/// A Become from S between two Becomes from any state: an agent in B is decided by the two from any state alone, A
/// with chance 0.2 and C with 0.8 times 0.5 each time round, and an agent in S by all three.
#[test]
fn a_become_from_a_state_between_two_from_any_state_never_decides_another_state() {
    let lines = [
        ("l2", "any_state", "a", 0.2),
        ("l3", "s", "b", 0.3),
        ("l4", "any_state", "c", 0.5),
    ];
    let rounds = (1.0 - 0.4_f64.powi(4)) / 0.6;
    let from_b = becomes_by_chance("b", &lines);
    assert_shares(
        &four_shares(&from_b),
        &[0.0, 0.2 * rounds, 0.4_f64.powi(4), 0.4 * rounds],
        HOST_AGENTS,
        "from B",
    );
    let rounds = (1.0 - 0.28_f64.powi(4)) / 0.72;
    let from_s = becomes_by_chance("s", &lines);
    assert_shares(
        &four_shares(&from_s),
        &[0.28_f64.powi(4), 0.2 * rounds, 0.24 * rounds, 0.28 * rounds],
        HOST_AGENTS,
        "from S",
    );
}

/// Sixteen competing Becomes fill the sixteen accumulators of one Repeat, one each, and the graph builds.
#[test]
fn sixteen_competing_becomes_take_one_accumulator_each() {
    let ids: Vec<String> = (2..18).map(|number| format!("l{number}")).collect();
    let lines: Vec<(&str, &str, &str, f64)> = ids.iter().map(|id| (id.as_str(), "s", "a", 0.1)).collect();
    let file = lowered(&becomes_by_chance("s", &lines));
    assert_eq!(repeat_accumulators(&file).len(), 16);
    host_of(&file);
}

/// A Become with chance 0.1 above one with chance 0.5 takes `0.1 (1 - 0.45^4) / 0.55` of the agents and the lower
/// `0.45 (1 - 0.45^4) / 0.55`. Under the any-time rule they took 0.344 and 0.615.
#[test]
fn becomes_by_chance_inside_a_repeat_follow_the_first_time_rule() {
    assert_shares(
        &status_shares(CHANCES),
        &[CHANCE_NONE, CHANCE_A, CHANCE_B],
        HOST_AGENTS,
        "chances",
    );
}

/// Two Sets of one Number property inside a Repeat follow the same rule as two Becomes.
#[test]
fn sets_inside_a_repeat_follow_the_first_time_rule() {
    let values = [HostValue::F32(0.0), HostValue::F32(1.0), HostValue::F32(2.0)];
    for text in [with_sets(ROLL), with_sets(&swapped_ids(ROLL))] {
        let shares = host_shares(&text, HOST_AGENTS, "level", &values);
        assert_shares(&shares, &[0.0, 0.5, 0.5], HOST_AGENTS, &text);
    }
    let shares = host_shares(&with_sets(CHANCES), HOST_AGENTS, "level", &values);
    assert_shares(&shares, &[CHANCE_NONE, CHANCE_A, CHANCE_B], HOST_AGENTS, "chances");
}

/// A Become from any state above or below a Become from S inside a Repeat: an agent in another state is decided by
/// the Become from any state alone, which applies when it held at any time round, and an agent in S splits evenly.
#[test]
fn a_become_from_a_state_never_overrules_an_agent_in_another_state() {
    let any_above = edited(
        ROLL,
        "new_state = \"a\"\nstart_state = \"s\"",
        "new_state = \"a\"\nstart_state = \"any_state\"",
    );
    let from_b = edited(&any_above, "value = { state = \"s\" }", "value = { state = \"b\" }");
    assert_shares(
        &status_shares(&from_b),
        &[0.0, 0.9375, 0.0625],
        HOST_AGENTS,
        "any above, from B",
    );
    assert_shares(
        &status_shares(&any_above),
        &[0.0, 0.5, 0.5],
        HOST_AGENTS,
        "any above, from S",
    );
    let any_below = edited(
        ROLL,
        "new_state = \"b\"\nstart_state = \"s\"",
        "new_state = \"b\"\nstart_state = \"any_state\"",
    );
    let from_a = edited(&any_below, "value = { state = \"s\" }", "value = { state = \"a\" }");
    assert_shares(
        &status_shares(&from_a),
        &[0.0, 0.0625, 0.9375],
        HOST_AGENTS,
        "any below, from A",
    );
    assert_shares(
        &status_shares(&any_below),
        &[0.0, 0.5, 0.5],
        HOST_AGENTS,
        "any below, from S",
    );
}

/// A Become outside a Repeat keeps display order against the Becomes inside it: above, it wins wherever it applies,
/// and below, it applies only where no line inside held.
#[test]
fn a_change_outside_a_repeat_keeps_display_order_against_one_inside() {
    let with_c = edited(
        CHANCES,
        "{ id = \"b\", name = \"B\", color = \"#E43748\" },",
        "{ id = \"b\", name = \"B\", color = \"#E43748\" },\n  { id = \"c\", name = \"C\", color = \"#3D7DD5\" },",
    );
    let with_c = edited(&with_c, "next_line = 4", "next_line = 5");
    let outside = |lines: &str, condition: &str| {
        let text = edited(&with_c, "lines = [\"l1\"]", lines);
        format!(
            "{text}\n[card.line.l4]\nkind = \"become\"\nkey = 103\nproperty = \"status\"\nnew_state = \"c\"\n\
             start_state = \"s\"\n\n[card.line.l4.condition]\n{condition}\n"
        )
    };
    let values = [
        HostValue::Category(0),
        HostValue::Category(1),
        HostValue::Category(2),
        HostValue::Category(3),
    ];
    let above = outside("lines = [\"l4\", \"l1\"]", "kind = \"chance\"\nchance = 0.3");
    let shares = host_shares(&above, HOST_AGENTS, "status", &values);
    assert_shares(
        &shares,
        &[0.7 * CHANCE_NONE, 0.7 * CHANCE_A, 0.7 * CHANCE_B, 0.3],
        HOST_AGENTS,
        "outside above",
    );
    let below = outside("lines = [\"l1\", \"l4\"]", "kind = \"always\"");
    let shares = host_shares(&below, HOST_AGENTS, "status", &values);
    assert_shares(
        &shares,
        &[0.0, CHANCE_A, CHANCE_B, CHANCE_NONE],
        HOST_AGENTS,
        "outside below",
    );
}

/// On the device, the chances sheet and the roll sheet over 200,000 agents take their closed-form shares after one
/// tick, within five standard errors.
#[test]
fn becomes_inside_a_repeat_take_the_first_time_shares_on_the_device() {
    let Some(device) = device_for("becomes_inside_a_repeat_take_the_first_time_shares_on_the_device") else {
        return;
    };
    let ctx = context(device);
    let agents = 200_000;
    for (text, expected) in [(CHANCES, [CHANCE_NONE, CHANCE_A, CHANCE_B]), (ROLL, [0.0, 0.5, 0.5])] {
        let file = lowered(text);
        let program = agent_program_of(&file);
        let status = program.lane_buffer("status").expect("the status lane");
        let entry = program.entry();
        let params = params_of(&entry, &[("num_agents", ParamValue::U32(agents))]);
        let mut state = built(&entry, &params, Some(SEED), &ctx);
        henad_compute::gpu::stepping::run_steps(&mut *state, &ctx, 1).expect("a tick runs");
        let words = state.read_buffer(status.buffer()).expect("the status lane reads back");
        let words = status.segment(&words);
        assert_eq!(words.len(), agents as usize);
        let shares: Vec<f64> = (0..3)
            .map(|state| words.iter().filter(|&&word| word == state).count() as f64 / f64::from(agents))
            .collect();
        assert_shares(&shares, &expected, agents, text);
    }
}
