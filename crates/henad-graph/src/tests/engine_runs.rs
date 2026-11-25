//! Runs of graphs through the GPU engine at the edges of its words and of a submission: a counter past 2^32, a crowd
//! in one cell of the neighbour index, and graphs with nothing to measure.

use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::stepping::{run_steps, sample_stats, submit_slice, wait_for};
use henad_compute::gpu::{GpuContext, MAX_STEPS_PER_SUBMISSION, SUBMISSION_WORK_BUDGET, wgpu};
use henad_core::params::ParamValue;
use henad_core::view::{StatEntry, StatValue};

use super::support::{built, context, device_for, params_of};
use crate::GraphFile;
use crate::wgsl::{AgentProgram, GridProgram};

/// A world of agents that each add 5000 to the counter `events` every tick, with the counter's total as its one stat.
const FIVE_THOUSAND_EACH: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "five_thousand_each"
name = "Five thousand each"

[graph]
topology = "agents"
library = 1

[[target]]
backend = "gpu"
id = "five_thousand_each"
name = "Five thousand each"

[agents]
count = 1000
max_count = 1000000
width = 32.0
height = 32.0

[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
initial = { vec2f = [1.5, 1.5] }
role = "position"

[[counter]]
id = "events"

[[stat]]
label = "events"
from = "n30"
color = [0, 0, 0, 255]

[[pass]]
id = "walk"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n17"
op = "counter.add"
config.counter = "events"
in.amount = { u32 = 5000 }

[[pass]]
id = "totals"
stage = "stats"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n30"
op = "stat.counter"
config.counter = "events"
"#;

/// Returns the value of the one stat of `stats`.
fn only_stat(stats: &[StatEntry]) -> f64 {
    match stats {
        [entry] => match entry.value() {
            StatValue::Scalar(value) => *value,
            other => panic!("the stat is {other:?}"),
        },
        _ => panic!("{} stats", stats.len()),
    }
}

/// A million agents that add 5000 each pass 2^32 in the first tick, and the stat reads the whole total after each tick
/// through the engine's counters, low word and high word.
#[test]
fn a_counter_past_2_32_reads_its_whole_total_through_the_engine() {
    let Some(device) = device_for("a_counter_past_2_32_reads_its_whole_total_through_the_engine") else {
        return;
    };
    let ctx = context(device);
    let file = GraphFile::parse(FIVE_THOUSAND_EACH).expect("the file reads");
    let entry = AgentProgram::compile(&file)
        .unwrap_or_else(|problems| panic!("{problems:#?}"))
        .entry();
    let params = params_of(&entry, &[("num_agents", ParamValue::U32(1_000_000))]);
    let mut state = built(&entry, &params, Some(7), &ctx);
    for tick in 1..=2u32 {
        run_steps(&mut *state, &ctx, 1).unwrap_or_else(|fault| panic!("tick {tick}: {fault}"));
        let stats = sample_stats(&mut *state, &ctx).unwrap_or_else(|fault| panic!("tick {tick}: {fault}"));
        let expected = 5_000_000_000.0 * f64::from(tick);
        assert_eq!(only_stat(&stats), expected, "the total after tick {tick}");
    }
}

/// A graph whose step work does not depend on where its agents stand submits 64 steps from its first submission, and
/// never waits for a measurement: the Game of Life starter on a grid, and agents with no radius fold.
#[test]
fn a_graph_with_nothing_to_measure_submits_64_steps_from_the_start() {
    let Some(device) = device_for("a_graph_with_nothing_to_measure_submits_64_steps_from_the_start") else {
        return;
    };
    let ctx = context(device);
    let life = crate::starters::starter("game_of_life.toml").expect("the Game of Life starter");
    let life = GridProgram::compile(&GraphFile::parse(life.text).expect("the starter reads"))
        .unwrap_or_else(|problems| panic!("{problems:#?}"))
        .entry();
    let agents = AgentProgram::compile(&GraphFile::parse(FIVE_THOUSAND_EACH).expect("the file reads"))
        .unwrap_or_else(|problems| panic!("{problems:#?}"))
        .entry();
    for entry in [life, agents] {
        let mut state = built(&entry, &params_of(&entry, &[]), Some(7), &ctx);
        assert_eq!(
            state.submission_steps(false),
            MAX_STEPS_PER_SUBMISSION,
            "{} at tick 0",
            entry.id()
        );
        run_steps(&mut *state, &ctx, 1).unwrap_or_else(|fault| panic!("{}: {fault}", entry.id()));
        assert_eq!(
            state.submission_steps(false),
            MAX_STEPS_PER_SUBMISSION,
            "{} after a step",
            entry.id()
        );
    }
}

/// A world of agents placed at one point, or scattered when `scatter` holds, that each count the agents within 5
/// units over an index of cells 5 units wide, then move right by a quarter of that count and up by a half. Agent 0
/// alone looks when `everyone` is false, and the work charged for the walks still counts every agent's walk. The action
/// `place_again` reruns the placement.
const ONE_POINT: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "one_point"
name = "One point"

[graph]
topology = "agents"
library = 1

[[target]]
backend = "gpu"
id = "one_point"
name = "One point"

[agents]
count = 1000
max_count = 60000
width = 320.0
height = 320.0

[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
initial = { vec2f = [0.0, 0.0] }
role = "position"

[[lane]]
id = "near"
domain = "agents"
type = "u32"
initial = { u32 = 0 }

[[param]]
id = "scatter"
label = "Scatter"
kind = "bool"
default = false

[[param]]
id = "everyone"
label = "Everyone looks"
kind = "bool"
default = true

[[action]]
id = "place_again"
label = "Place again"
runs = ["place"]

[[pass]]
id = "place"
stage = "init"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n1"
op = "ctx.index"

[[pass.node]]
id = "n2"
op = "rng.hash"
in.x = "n1"

[[pass.node]]
id = "n3"
op = "rng.hash"
in.x = "n2"

[[pass.node]]
id = "n4"
op = "rng.to_float"
in.bits = "n2"
in.max = { f32 = 320.0 }

[[pass.node]]
id = "n5"
op = "rng.to_float"
in.bits = "n3"
in.max = { f32 = 320.0 }

[[pass.node]]
id = "n6"
op = "vec.make"
in.x = "n4"
in.y = "n5"

[[pass.node]]
id = "n7"
op = "logic.select"
in.cond = { param = "scatter" }
in.if_true = "n6"
in.if_false = { vec2f = [160.5, 160.5] }

[[pass.node]]
id = "n10"
op = "lane.set"
config.lane = "pos"
in.value = "n7"

[[pass]]
id = "look"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n11"
op = "ctx.index"

[[pass.node]]
id = "n12"
op = "cmp.eq"
in.a = "n11"
in.b = { u32 = 0 }

[[pass.node]]
id = "n13"
op = "logic.or"
in.a = { param = "everyone" }
in.b = "n12"

[[pass.node]]
id = "n15"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n16"
op = "agent.radius_fold"
config.exclude_self = true
config.acc = [{ name = "count", type = "u32", combine = "count" }]
in.center = "n15"
in.radius = { f32 = 5.0 }
in.cell_size = { f32 = 5.0 }
in.when = "n13"

[[pass.node]]
id = "n17"
op = "region.put"
region = "n16"
config.acc = "count"
in.when = { bool = true }

[[pass.node]]
id = "n18"
op = "lane.set"
config.lane = "near"
in.value = "n16.count"

[[pass.node]]
id = "n19"
op = "conv.to_f32"
in.a = "n16.count"

[[pass.node]]
id = "n20"
op = "math.mul"
in.a = "n19"
in.b = { f32 = 0.25 }

[[pass.node]]
id = "n21"
op = "vec.make"
in.x = "n20"
in.y = { f32 = 0.5 }

[[pass.node]]
id = "n22"
op = "math.add"
in.a = "n15"
in.b = "n21"

[[pass.node]]
id = "n23"
op = "ctx.world"

[[pass.node]]
id = "n24"
op = "space.wrap"
in.v = "n22"
in.world = "n23"

[[pass.node]]
id = "n25"
op = "lane.set"
config.lane = "pos"
in.value = "n24"
"#;

/// Agents of [`ONE_POINT`] in its clustered start, enough that a submission of 64 steps at their measured work
/// passes the budget, and few enough that every walk stays under the visit cap. Agent 0 alone looks, so a step walks
/// the crowd once while the charge counts a walk for every agent.
const CROWD: u32 = 40_000;

/// Agents of [`ONE_POINT`] in its scattered start, where every agent looks and a submission holds 64 steps.
const SCATTERED: u32 = 3_000;

/// Returns the program of [`ONE_POINT`] and its parameters at `agents` agents, placed at one point or scattered, with
/// every agent looking or agent 0 alone.
fn one_point(agents: u32, scatter: bool, everyone: bool) -> (AgentProgram, Vec<ParamValue>) {
    let file = GraphFile::parse(ONE_POINT).expect("the file reads");
    let program = AgentProgram::compile(&file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let params = params_of(
        &program.entry(),
        &[
            ("num_agents", ParamValue::U32(agents)),
            ("scatter", ParamValue::Bool(scatter)),
            ("everyone", ParamValue::Bool(everyone)),
        ],
    );
    (program, params)
}

/// Submits one step of `state` and waits for it.
fn one_step(state: &mut dyn GpuSimState, ctx: &GpuContext) {
    submit_slice(state, ctx, 1, false);
    wait_for(state, ctx).unwrap_or_else(|fault| panic!("{fault}"));
}

/// A crowd that starts in one cell of the index passes the build at the charge of an even spread, runs its first step
/// alone, and its second submission holds the steps its measured largest bucket allows, fewer than 64.
#[test]
fn a_crowd_in_one_cell_sizes_the_second_submission_from_its_measurement() {
    let Some(device) = device_for("a_crowd_in_one_cell_sizes_the_second_submission_from_its_measurement") else {
        return;
    };
    let ctx = context(device);
    let (program, params) = one_point(CROWD, false, false);
    let spec = program.spec();
    let limits = ctx.device.limits();
    assert!(
        spec.demand(&params, &limits).submission_work() <= SUBMISSION_WORK_BUDGET,
        "the build is charged for an even spread"
    );
    let expected = spec.submission_steps(&params, &limits, CROWD);
    assert!(
        (2..MAX_STEPS_PER_SUBMISSION).contains(&expected),
        "a crowd in one cell holds {expected} steps a submission"
    );

    let mut state = built(&program.entry(), &params, Some(7), &ctx);
    assert_eq!(state.submission_steps(true), 1, "the first step goes alone");
    one_step(&mut *state, &ctx);
    assert_eq!(
        state.submission_steps(true),
        expected,
        "the second submission is sized from every agent in one cell"
    );
}

/// A scattered start keeps 64 steps a submission once measured, and a press sends the next step alone and measures
/// again after it.
#[test]
fn a_press_measures_the_index_again_after_the_next_step() {
    let Some(device) = device_for("a_press_measures_the_index_again_after_the_next_step") else {
        return;
    };
    let ctx = context(device);
    let (program, params) = one_point(SCATTERED, true, true);
    let mut state = built(&program.entry(), &params, Some(7), &ctx);
    assert_eq!(state.submission_steps(true), 1, "the first step goes alone");
    one_step(&mut *state, &ctx);
    assert_eq!(
        state.submission_steps(true),
        MAX_STEPS_PER_SUBMISSION,
        "a scattered start"
    );

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    assert!(state.encode_action(&mut encoder, 0), "the action runs");
    ctx.queue.submit(Some(encoder.finish()));
    assert_eq!(state.submission_steps(true), 1, "the step after a press goes alone");
    one_step(&mut *state, &ctx);
    assert_eq!(state.submission_steps(true), MAX_STEPS_PER_SUBMISSION, "measured again");
}

/// A measurement in flight across a press read the index before the press and sizes nothing: the step after the press
/// goes alone, and the measurement after that step sizes the submissions.
#[test]
fn a_measurement_from_before_a_press_sizes_nothing() {
    let Some(device) = device_for("a_measurement_from_before_a_press_sizes_nothing") else {
        return;
    };
    let ctx = context(device);
    let (program, params) = one_point(SCATTERED, true, true);
    let mut state = built(&program.entry(), &params, Some(7), &ctx);
    submit_slice(&mut *state, &ctx, 1, true);
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    assert!(state.encode_action(&mut encoder, 0), "the action runs");
    ctx.queue.submit(Some(encoder.finish()));
    assert_eq!(
        state.submission_steps(true),
        1,
        "the sample measured the index before the press"
    );
    one_step(&mut *state, &ctx);
    assert_eq!(
        state.submission_steps(true),
        MAX_STEPS_PER_SUBMISSION,
        "measured after it"
    );
}

/// A stats sample after the first step carries the measurement of the index, so the next submission is sized without
/// a wait of its own.
#[test]
fn a_stats_readback_carries_the_measurement() {
    let Some(device) = device_for("a_stats_readback_carries_the_measurement") else {
        return;
    };
    let ctx = context(device);
    let (program, params) = one_point(SCATTERED, true, true);
    let mut state = built(&program.entry(), &params, Some(7), &ctx);
    assert_eq!(state.submission_steps(false), 1, "the first step goes alone");
    submit_slice(&mut *state, &ctx, 1, true);
    state.poll_stats_readback(&ctx.device, true);
    assert_eq!(
        state.submission_steps(false),
        MAX_STEPS_PER_SUBMISSION,
        "the sample measured the index"
    );
}

/// Ticks in submissions sized from the measured index leave every buffer as ticks submitted one at a time do, word for
/// word. A fold's count does not depend on the order the index lists agents in, so the run replays.
#[test]
fn the_submission_sizes_change_no_result() {
    let Some(device) = device_for("the_submission_sizes_change_no_result") else {
        return;
    };
    let ctx = context(device);
    let (program, params) = one_point(SCATTERED, true, true);
    let entry = program.entry();
    let ticks = 70;
    let mut sized = built(&entry, &params, Some(7), &ctx);
    run_steps(&mut *sized, &ctx, ticks).unwrap_or_else(|fault| panic!("{fault}"));
    assert_eq!(
        sized.submission_steps(false),
        MAX_STEPS_PER_SUBMISSION,
        "a measurement sized the run"
    );
    let mut single = built(&entry, &params, Some(7), &ctx);
    for _ in 0..ticks {
        submit_slice(&mut *single, &ctx, 1, false);
    }
    wait_for(&mut *single, &ctx).unwrap_or_else(|fault| panic!("{fault}"));
    assert_eq!((sized.tick(), single.tick()), (ticks, ticks));
    let mut buffer = 0;
    while let Some(words) = sized.read_buffer(buffer) {
        assert_eq!(Some(words), single.read_buffer(buffer), "buffer {buffer}");
        buffer += 1;
    }
    assert!(buffer > 1, "the lanes read back");
}
