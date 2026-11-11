//! The Repeat tests of the plan's "Finite loops" section over agents, built through the model file and stepped on a
//! device, each held to the host evaluator's run of the same passes.
//!
//! The graphs place agent `i` at the centre of cell `(i % 40, (i / 40) % 30)` of a 40 by 30 world, so each expected
//! value has a closed form. The random walk starter is the corpus file of agents that takes a Repeat.

use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_core::authoring::primitives::rng::pcg_hash;
use henad_core::params::ParamValue;
use henad_graph::GraphFile;
use henad_graph::document::PassStage;
use henad_graph::host::{HostEvaluator, HostWorld, RunContext};

use super::lanes::GraphLanes;
use super::support::{
    baseline_device, build_gpu, lane, model_entry, pairs, params_for, roomy_limits, starter_entry, step,
};

/// Number of agents of every test graph.
const AGENTS: u32 = 300;
/// Columns and rows of the test graphs' world, one cell a unit.
const SIDES: (u32, u32) = (40, 30);

/// Head of every test graph: an agents world, the position lane, and an init pass that places each agent at the
/// centre of its cell.
const HEAD: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_agent_repeat"

[graph]
topology = "agents"
library = 1

[[target]]
backend = "gpu"

[agents]
count = 300
max_count = 100000
width = 40.0
height = 30.0

[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

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
op = "math.rem"
in.a = "n1"
in.b = { u32 = 40 }

[[pass.node]]
id = "n3"
op = "math.div"
in.a = "n1"
in.b = { u32 = 40 }

[[pass.node]]
id = "n4"
op = "math.rem"
in.a = "n3"
in.b = { u32 = 30 }

[[pass.node]]
id = "n5"
op = "conv.to_f32"
in.a = "n2"

[[pass.node]]
id = "n6"
op = "conv.to_f32"
in.a = "n4"

[[pass.node]]
id = "n7"
op = "math.add"
in.a = "n5"
in.b = { f32 = 0.5 }

[[pass.node]]
id = "n8"
op = "math.add"
in.a = "n6"
in.b = { f32 = 0.5 }

[[pass.node]]
id = "n9"
op = "vec.make"
in.x = "n7"
in.y = "n8"

[[pass.node]]
id = "n10"
op = "lane.set"
config.lane = "pos"
in.value = "n9"
"#;

/// Returns the declaration of the `u32` lane `id` over agents.
fn u32_lane(id: &str) -> String {
    format!("\n[[lane]]\nid = \"{id}\"\ndomain = \"agents\"\ntype = \"u32\"\n")
}

/// Returns the declaration of the `f32` lane `id` over agents.
fn f32_lane(id: &str) -> String {
    format!("\n[[lane]]\nid = \"{id}\"\ndomain = \"agents\"\ntype = \"f32\"\n")
}

/// The known-answer cases of a Repeat, one lane each: zero iterations (`zero`), `when` false (`off`), a count clamped
/// to `max` (`clamped`), a count per agent (`own`), and an inner count that reads the outer `k` (`nested`).
const ANSWERS: &str = r#"
[[pass]]
id = "answers"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n20"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "v", type = "u32", combine = "carry" }]
in.count = { u32 = 0 }
in.init.v = { u32 = 7 }

[[pass.node]]
id = "n21"
op = "math.add"
in.a = "n20.prev.v"
in.b = { u32 = 1 }
region = "n20"

[[pass.node]]
id = "n22"
op = "region.put"
config.acc = "v"
in.value = "n21"
region = "n20"

[[pass.node]]
id = "n23"
op = "lane.set"
config.lane = "zero"
in.value = "n20.v"

[[pass.node]]
id = "n24"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "v", type = "u32", combine = "carry" }]
in.count = { u32 = 5 }
in.when = { bool = false }
in.init.v = { u32 = 7 }

[[pass.node]]
id = "n25"
op = "math.add"
in.a = "n24.prev.v"
in.b = { u32 = 1 }
region = "n24"

[[pass.node]]
id = "n26"
op = "region.put"
config.acc = "v"
in.value = "n25"
region = "n24"

[[pass.node]]
id = "n27"
op = "lane.set"
config.lane = "off"
in.value = "n24.v"

[[pass.node]]
id = "n28"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "sum", type = "u32", combine = "sum" }]
in.count = { u32 = 100 }

[[pass.node]]
id = "n29"
op = "region.put"
config.acc = "sum"
in.value = "n28.k"
region = "n28"

[[pass.node]]
id = "n30"
op = "lane.set"
config.lane = "clamped"
in.value = "n28.sum"

[[pass.node]]
id = "n31"
op = "ctx.index"

[[pass.node]]
id = "n32"
op = "math.rem"
in.a = "n31"
in.b = { u32 = 6 }

[[pass.node]]
id = "n33"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "sum", type = "u32", combine = "sum" }]
in.count = "n32"

[[pass.node]]
id = "n34"
op = "region.put"
config.acc = "sum"
in.value = "n33.k"
region = "n33"

[[pass.node]]
id = "n35"
op = "lane.set"
config.lane = "own"
in.value = "n33.sum"

[[pass.node]]
id = "n36"
op = "math.rem"
in.a = "n31"
in.b = { u32 = 4 }

[[pass.node]]
id = "n37"
op = "region.repeat"
config.max = 4
config.acc = [{ name = "sum", type = "u32", combine = "sum" }]
in.count = "n36"

[[pass.node]]
id = "n38"
op = "math.add"
in.a = "n37.k"
in.b = { u32 = 1 }
region = "n37"

[[pass.node]]
id = "n39"
op = "region.repeat"
config.max = 5
config.acc = [{ name = "steps", type = "u32", combine = "count" }]
in.count = "n38"
region = "n37"

[[pass.node]]
id = "n40"
op = "region.put"
config.acc = "steps"
region = "n39"

[[pass.node]]
id = "n41"
op = "region.put"
config.acc = "sum"
in.value = "n39.steps"
region = "n37"

[[pass.node]]
id = "n42"
op = "lane.set"
config.lane = "nested"
in.value = "n37.sum"
"#;

/// Two iterations of one draw, the first iteration's bits in the lane `first` and the second's in `second`.
const DRAWS: &str = r#"
[[pass]]
id = "draws"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n60"
op = "region.repeat"
config.max = 2
config.acc = [
  { name = "first", type = "u32", combine = "carry" },
  { name = "second", type = "u32", combine = "carry" },
]
in.count = { u32 = 2 }
in.init.first = { u32 = 0 }
in.init.second = { u32 = 0 }

[[pass.node]]
id = "n61"
op = "rand.bits"
config.key = 3141592653
region = "n60"

[[pass.node]]
id = "n62"
op = "cmp.eq"
in.a = "n60.k"
in.b = { u32 = 0 }
region = "n60"

[[pass.node]]
id = "n63"
op = "logic.not"
in.a = "n62"
region = "n60"

[[pass.node]]
id = "n64"
op = "region.put"
config.acc = "first"
in.value = "n61"
in.when = "n62"
region = "n60"

[[pass.node]]
id = "n65"
op = "region.put"
config.acc = "second"
in.value = "n61"
in.when = "n63"
region = "n60"

[[pass.node]]
id = "n66"
op = "lane.set"
config.lane = "first"
in.value = "n60.first"

[[pass.node]]
id = "n67"
op = "lane.set"
config.lane = "second"
in.value = "n60.second"
"#;

/// Number of links of the hash chain.
const CHAIN_LINKS: usize = 5;

/// A deterministic body, `rng.hash` of the previous value from the agent's index, carried through a Repeat of
/// [`CHAIN_LINKS`] iterations into the lane `chain`.
const CHAIN_LOOP: &str = r#"
[[pass]]
id = "chain"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n50"
op = "ctx.index"

[[pass.node]]
id = "n51"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "h", type = "u32", combine = "carry" }]
in.count = { u32 = 5 }
in.init.h = "n50"

[[pass.node]]
id = "n52"
op = "rng.hash"
in.x = "n51.prev.h"
region = "n51"

[[pass.node]]
id = "n53"
op = "region.put"
config.acc = "h"
in.value = "n52"
region = "n51"

[[pass.node]]
id = "n54"
op = "lane.set"
config.lane = "chain"
in.value = "n51.h"
"#;

/// Returns the pass of [`CHAIN_LOOP`] with its body copied [`CHAIN_LINKS`] times by hand, outside any region.
fn chain_copies() -> String {
    let mut text = String::from("\n[[pass]]\nid = \"chain\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n");
    text.push_str("\n[[pass.node]]\nid = \"n50\"\nop = \"ctx.index\"\n");
    let mut previous = "n50".to_owned();
    for link in 0..CHAIN_LINKS {
        let id = format!("n{}", 100 + link);
        text.push_str(&format!(
            "\n[[pass.node]]\nid = \"{id}\"\nop = \"rng.hash\"\nin.x = \"{previous}\"\n"
        ));
        previous = id;
    }
    text.push_str(&format!(
        "\n[[pass.node]]\nid = \"n54\"\nop = \"lane.set\"\nconfig.lane = \"chain\"\nin.value = \"{previous}\"\n"
    ));
    text
}

/// Returns the graph of every Repeat test of this module but the folds, with `chain` as the pass of the hash chain.
fn repeats_text(chain: &str) -> String {
    let lanes: String = ["zero", "off", "clamped", "own", "nested", "first", "second", "chain"]
        .iter()
        .map(|id| u32_lane(id))
        .collect();
    format!("{HEAD}{lanes}{ANSWERS}{DRAWS}{chain}")
}

/// A Repeat of three iterations holding a `grid.fold` centred `k` columns to the right of the agent's cell, summing a
/// static field into the lane `folded`, then a `grid.fold` holding a Repeat of two iterations, which adds each value
/// twice into the lane `inner`. Both folds walk the 3 by 3 stencil on the torus, centre included.
const FOLDS: &str = r#"
[[field]]
id = "terrain"
type = "f32"
layers = ["value"]
static = true
initial = { f32 = 0.0 }

[[pass]]
id = "terrain"
stage = "init"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n70"
op = "ctx.index"

[[pass.node]]
id = "n71"
op = "math.rem"
in.a = "n70"
in.b = { u32 = 7 }

[[pass.node]]
id = "n72"
op = "conv.to_f32"
in.a = "n71"

[[pass.node]]
id = "n73"
op = "field.set"
config.field = "terrain"
in.layer = { layer = "terrain.value" }
in.value = "n72"

[[pass]]
id = "folds"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n80"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n81"
op = "space.cell_of"
in.p = "n80"

[[pass.node]]
id = "n82"
op = "region.repeat"
config.max = 3
config.acc = [{ name = "sum", type = "f32", combine = "sum" }]
in.count = { u32 = 3 }

[[pass.node]]
id = "n83"
op = "conv.to_i32"
in.a = "n82.k"
region = "n82"

[[pass.node]]
id = "n84"
op = "vec.make"
in.x = "n83"
in.y = { i32 = 0 }
region = "n82"

[[pass.node]]
id = "n85"
op = "math.add"
in.a = "n81.xy"
in.b = "n84"
region = "n82"

[[pass.node]]
id = "n86"
op = "grid.fold"
config.order = "moore_row_major"
config.include_center = true
config.boundary = "torus"
config.acc = [{ name = "total", type = "f32", combine = "sum" }]
in.at = "n85"
region = "n82"

[[pass.node]]
id = "n87"
op = "field.get"
config.field = "terrain"
in.layer = { layer = "terrain.value" }
in.at = "n86.cell"
region = "n86"

[[pass.node]]
id = "n88"
op = "region.put"
config.acc = "total"
in.value = "n87"
region = "n86"

[[pass.node]]
id = "n89"
op = "region.put"
config.acc = "sum"
in.value = "n86.total"
region = "n82"

[[pass.node]]
id = "n90"
op = "lane.set"
config.lane = "folded"
in.value = "n82.sum"

[[pass.node]]
id = "n91"
op = "grid.fold"
config.order = "moore_row_major"
config.include_center = true
config.boundary = "torus"
config.acc = [{ name = "total", type = "f32", combine = "sum" }]
in.at = "n81.xy"

[[pass.node]]
id = "n92"
op = "field.get"
config.field = "terrain"
in.layer = { layer = "terrain.value" }
in.at = "n91.cell"
region = "n91"

[[pass.node]]
id = "n93"
op = "region.repeat"
config.max = 2
config.acc = [{ name = "twice", type = "f32", combine = "sum" }]
in.count = { u32 = 2 }
region = "n91"

[[pass.node]]
id = "n94"
op = "region.put"
config.acc = "twice"
in.value = "n92"
region = "n93"

[[pass.node]]
id = "n95"
op = "region.put"
config.acc = "total"
in.value = "n93.twice"
region = "n91"

[[pass.node]]
id = "n96"
op = "lane.set"
config.lane = "inner"
in.value = "n91.total"
"#;

/// Returns the graph of the two nested folds.
fn folds_text() -> String {
    format!("{HEAD}{}{}{FOLDS}", f32_lane("folded"), f32_lane("inner"))
}

/// Returns the file of `text`, checked without errors.
fn checked(text: &str) -> GraphFile {
    let file = GraphFile::parse(text).unwrap_or_else(|error| panic!("{error}\n{text}"));
    let report = file.check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    file
}

/// Returns the engine's parameters of the test world.
fn world_params() -> [ParamValue; 3] {
    [
        ParamValue::U32(AGENTS),
        ParamValue::F32(SIDES.0 as f32),
        ParamValue::F32(SIDES.1 as f32),
    ]
}

/// Returns every lane of `text` after `ticks` steps on the device from seed `seed`, by lane id, the position lane
/// left out.
fn on_device(ctx: &GpuContext, text: &str, seed: Option<u64>, ticks: u32) -> Vec<(String, Vec<u32>)> {
    let file = checked(text);
    let mut state = build_gpu(&model_entry(text), &world_params(), seed, ctx);
    step(ctx, &mut *state, ticks);
    lanes_of(&file, &*state)
}

/// Returns the lanes of `file` that `state` holds, by lane id, the position lane left out.
fn lanes_of(file: &GraphFile, state: &dyn GpuSimState) -> Vec<(String, Vec<u32>)> {
    let lanes = GraphLanes::of(file);
    file.document()
        .lanes
        .iter()
        .skip(1)
        .map(|lane| (lane.id.clone(), lanes.read(state, &lane.id)))
        .collect()
}

/// Returns every lane of `text` after one step on the host from seed `seed`, by lane id, the position lane left out.
fn on_host(text: &str, seed: Option<u64>) -> Vec<(String, Vec<u32>)> {
    let file = checked(text);
    let evaluator = HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{:?}", error.diagnostics()));
    let world = HostWorld::Agents {
        count: AGENTS,
        width: SIDES.0 as f32,
        height: SIDES.1 as f32,
    };
    let mut lanes = evaluator.init(world, &[], seed).expect("the init runs");
    for pass in evaluator
        .document()
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::Tick)
    {
        let mut stream = 0;
        evaluator
            .run_pass(&pass.id, &mut lanes, &[], &RunContext::tick(seed, 0), &mut stream)
            .unwrap_or_else(|error| panic!("{}: {error}", pass.id));
    }
    file.document()
        .lanes
        .iter()
        .skip(1)
        .map(|lane| {
            (
                lane.id.clone(),
                lanes.column(&lane.id).expect("the lane").words().to_vec(),
            )
        })
        .collect()
}

/// The known-answer cases of a Repeat hold over agents on the device and on the host: zero iterations and `when` false
/// return the initial value, a count of 100 runs `max` 8 times, a count per agent runs that many times, and an inner
/// count that reads the outer `k` runs `k + 1` times.
#[test]
fn a_repeat_over_agents_meets_its_known_answers() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let text = repeats_text(CHAIN_LOOP);
    let device = on_device(&ctx, &text, None, 1);
    let host = on_host(&text, None);
    for agents in [&device, &host] {
        for i in 0..AGENTS as usize {
            let own = i as u32 % 6;
            let nested = i as u32 % 4;
            assert_eq!(lane(agents, "zero")[i], 7, "zero iterations, agent {i}");
            assert_eq!(lane(agents, "off")[i], 7, "`when` false, agent {i}");
            assert_eq!(lane(agents, "clamped")[i], 28, "a clamped count, agent {i}");
            assert_eq!(
                lane(agents, "own")[i],
                own * own.saturating_sub(1) / 2,
                "agent {i}'s own count"
            );
            assert_eq!(
                lane(agents, "nested")[i],
                nested * (nested + 1) / 2,
                "agent {i}'s nested count"
            );
        }
    }
}

/// A deterministic body over five iterations, a hash chain, equals five hand-chained copies of the body bit for bit
/// on the device, both equal the host's run, and every agent's chain is `pcg_hash` applied five times to its index.
#[test]
fn a_repeat_over_agents_equals_its_hand_chained_copies_bit_for_bit() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let looped = repeats_text(CHAIN_LOOP);
    let copied = repeats_text(&chain_copies());
    let device = on_device(&ctx, &looped, Some(3), 1);
    assert_eq!(device, on_device(&ctx, &copied, Some(3), 1), "the loop and its copies");
    assert_eq!(device, on_host(&looped, Some(3)), "the device and the host");
    assert_eq!(
        on_host(&copied, Some(3)),
        on_host(&looped, Some(3)),
        "the host's loop and copies"
    );
    for (i, &found) in lane(&device, "chain").iter().enumerate() {
        let expected = (0..CHAIN_LINKS).fold(i as u32, |word, _| pcg_hash(word));
        assert_eq!(found, expected, "agent {i}");
    }
}

/// A deterministic float body, `prev * 1.1 + k` from the agent's index, carried through a Repeat of [`CHAIN_LINKS`]
/// iterations into the lane `number`.
const FLOAT_LOOP: &str = r#"
[[pass]]
id = "float"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n50"
op = "ctx.index"

[[pass.node]]
id = "n51"
op = "conv.to_f32"
in.a = "n50"

[[pass.node]]
id = "n52"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "number", type = "f32", combine = "carry" }]
in.count = { u32 = 5 }
in.init.number = "n51"

[[pass.node]]
id = "n53"
op = "math.mul"
in.a = "n52.prev.number"
in.b = { f32 = 1.1 }
region = "n52"

[[pass.node]]
id = "n54"
op = "conv.to_f32"
in.a = "n52.k"
region = "n52"

[[pass.node]]
id = "n55"
op = "math.add"
in.a = "n53"
in.b = "n54"
region = "n52"

[[pass.node]]
id = "n56"
op = "region.put"
config.acc = "number"
in.value = "n55"
region = "n52"

[[pass.node]]
id = "n57"
op = "lane.set"
config.lane = "number"
in.value = "n52.number"
"#;

/// Returns the graph of a float pass: [`FLOAT_LOOP`], or its body copied [`CHAIN_LINKS`] times by hand with each `k`
/// a literal.
fn float_text(copied: bool) -> String {
    let pass = if copied {
        let mut text =
            String::from("\n[[pass]]\nid = \"float\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n");
        text.push_str("\n[[pass.node]]\nid = \"n50\"\nop = \"ctx.index\"\n");
        text.push_str("\n[[pass.node]]\nid = \"n51\"\nop = \"conv.to_f32\"\nin.a = \"n50\"\n");
        let mut previous = "n51".to_owned();
        for k in 0..CHAIN_LINKS {
            let (product, sum) = (100 + 2 * k, 101 + 2 * k);
            text.push_str(&format!(
                "\n[[pass.node]]\nid = \"n{product}\"\nop = \"math.mul\"\nin.a = \"{previous}\"\nin.b = {{ f32 = 1.1 }}\n"
            ));
            text.push_str(&format!(
                "\n[[pass.node]]\nid = \"n{sum}\"\nop = \"math.add\"\nin.a = \"n{product}\"\nin.b = {{ f32 = {k}.0 }}\n"
            ));
            previous = format!("n{sum}");
        }
        text.push_str(&format!(
            "\n[[pass.node]]\nid = \"n57\"\nop = \"lane.set\"\nconfig.lane = \"number\"\nin.value = \"{previous}\"\n"
        ));
        text
    } else {
        FLOAT_LOOP.to_owned()
    };
    format!("{HEAD}{}{pass}", f32_lane("number"))
}

/// The float body of [`FLOAT_LOOP`] replays on one device over agents: two runs of the loop leave the same words, as
/// do two runs of its hand-chained copies. The host runs the loop and the copies alike.
#[test]
fn a_float_repeat_over_agents_replays_on_the_device() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    for copied in [false, true] {
        let text = float_text(copied);
        let first = on_device(&ctx, &text, Some(3), 1);
        assert_eq!(first, on_device(&ctx, &text, Some(3), 1), "one device replays:\n{text}");
        assert_ne!(lane(&first, "number")[3], 3.0f32.to_bits(), "the body ran");
    }
    assert_eq!(
        on_host(&float_text(false), Some(3)),
        on_host(&float_text(true), Some(3)),
        "the host's loop and copies"
    );
}

/// The float body of [`FLOAT_LOOP`] over agents equals its hand-chained copies bit for bit on the device, and both
/// equal the host.
///
/// Note that this probes a device and gates nothing, as the grid version in henad-graph does. A device may fuse a
/// multiply with an add, or reorder float operations, and then rounds the loop and the copies, whose `k` are literals,
/// differently, or differently from the host. That happens on Apple Metal, which compiles with fast math, and on
/// lavapipe on aarch64, where LLVM contracts the multiply and the add.
#[test]
#[ignore = "probes a device that may fuse a multiply with an add or reorder float operations"]
fn a_float_repeat_over_agents_equals_its_hand_chained_copies_bit_for_bit() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let looped = on_device(&ctx, &float_text(false), Some(3), 1);
    assert_eq!(
        looped,
        on_device(&ctx, &float_text(true), Some(3), 1),
        "the loop and its copies"
    );
    assert_eq!(looped, on_host(&float_text(false), Some(3)), "the device and the host");
}

/// Two iterations of one draw differ for nearly every agent, two runs at one seed are equal, another seed draws
/// otherwise, and each draw on the device equals the host's.
#[test]
fn draws_in_a_repeat_over_agents_differ_by_iteration_and_replay() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let text = repeats_text(CHAIN_LOOP);
    let device = on_device(&ctx, &text, Some(5), 1);
    assert_eq!(device, on_host(&text, Some(5)), "the device and the host");
    let (first, second) = (lane(&device, "first"), lane(&device, "second"));
    let equal = first.iter().zip(second).filter(|(a, b)| a == b).count();
    assert!(equal <= 1, "{equal} agents drew the same bits in both iterations");
    assert_eq!(device, on_device(&ctx, &text, Some(5), 1), "two runs at one seed");
    let other = on_device(&ctx, &text, Some(6), 1);
    assert_ne!(lane(&other, "first"), first, "another seed");
}

/// Returns the terrain value of cell `(x, y)` of the test world, wrapped onto the torus.
fn terrain(x: i64, y: i64) -> f32 {
    let (width, height) = (i64::from(SIDES.0), i64::from(SIDES.1));
    let cell = y.rem_euclid(height) * width + x.rem_euclid(width);
    (cell % 7) as f32
}

/// Returns the sum of the terrain over the 3 by 3 stencil around `(x, y)`.
fn stencil(x: i64, y: i64) -> f32 {
    let mut total = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            total += terrain(x + dx, y + dy);
        }
    }
    total
}

/// A `grid.fold` nested in a Repeat, its centre moved by the Repeat's `k`, and a Repeat nested in a `grid.fold` give
/// their closed forms on the device and on the host, bit for bit. The field holds small integers, which every order
/// of addition sums exactly.
#[test]
fn folds_and_repeats_nest_over_agents() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let text = folds_text();
    let device = on_device(&ctx, &text, None, 1);
    assert_eq!(device, on_host(&text, None), "the device and the host");
    for i in 0..AGENTS {
        let (x, y) = (i64::from(i % SIDES.0), i64::from((i / SIDES.0) % SIDES.1));
        let folded = (0..3).map(|k| stencil(x + k, y)).sum::<f32>();
        assert_eq!(
            f32::from_bits(lane(&device, "folded")[i as usize]),
            folded,
            "agent {i}, the fold in a Repeat"
        );
        assert_eq!(
            f32::from_bits(lane(&device, "inner")[i as usize]),
            2.0 * stencil(x, y),
            "agent {i}, the Repeat in a fold"
        );
    }
}

/// The random walk starter replays at one seed, every agent's last step has the step size, and over a tick an agent
/// moves a mean squared distance of `sub_steps * step_size^2`, the closed form of a walk of uniform headings, within
/// five standard errors. With no steps a tick, the Repeat runs zero times and the walk stands still.
#[test]
fn the_random_walk_replays_and_steps_its_closed_form() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let entry = starter_entry("random_walk.toml");
    let lanes = GraphLanes::of_starter("random_walk.toml");
    let (step_size, side) = (2.0f32, 1_000.0f32);
    let params = params_for(&entry, &[("sub_steps", ParamValue::U32(4))]);
    let mut first = build_gpu(&entry, &params, Some(5), &ctx);
    let mut second = build_gpu(&entry, &params, Some(5), &ctx);
    step(&ctx, &mut *first, 3);
    step(&ctx, &mut *second, 3);
    for lane in ["pos", "last"] {
        assert!(
            lanes.read(&*first, lane) == lanes.read(&*second, lane),
            "the lane {lane} replays"
        );
    }
    let before = pairs(&lanes.read(&*first, "pos"));
    step(&ctx, &mut *first, 1);
    let after = pairs(&lanes.read(&*first, "pos"));
    let last = pairs(&lanes.read(&*first, "last"));
    for (i, &(x, y)) in last.iter().enumerate() {
        let length = x.hypot(y);
        assert!(
            (length - step_size).abs() <= 1e-5,
            "agent {i}'s last step is {length} long"
        );
    }
    let torus = |a: f32, b: f32| {
        let distance = (a - b).abs();
        f64::from(distance.min(side - distance))
    };
    let squares: Vec<f64> = before
        .iter()
        .zip(&after)
        .map(|(&(x0, y0), &(x1, y1))| torus(x0, x1).powi(2) + torus(y0, y1).powi(2))
        .collect();
    let mean = squares.iter().sum::<f64>() / squares.len() as f64;
    let expected = 4.0 * f64::from(step_size).powi(2);
    // The square of a sum of four unit-free steps of length L has variance 12 L^4: six pairs, each 2 L^2 cos.
    let standard_error = (12.0 * f64::from(step_size).powi(4) / squares.len() as f64).sqrt();
    assert!(
        (mean - expected).abs() <= 5.0 * standard_error,
        "the mean squared step over a tick is {mean}, and a walk of uniform headings takes {expected}"
    );
    let params = params_for(&entry, &[("sub_steps", ParamValue::U32(0))]);
    let mut still = build_gpu(&entry, &params, Some(5), &ctx);
    let start = lanes.read(&*still, "pos");
    step(&ctx, &mut *still, 2);
    assert!(lanes.read(&*still, "pos") == start, "zero steps a tick move nothing");
    assert!(
        lanes.read(&*still, "last").iter().all(|&word| word == 0),
        "zero steps a tick leave the last step at its initial value"
    );
}

/// A Repeat over agents of 16384 iterations, within the per-element limit, is refused by the submission budget at a
/// million agents, by the entry's shortfalls and by a build, before the engine allocates or seeds.
#[test]
fn a_repeat_over_agents_is_refused_by_the_submission_budget_at_a_large_population() {
    let clamped = "config.max = 8\nconfig.acc = [{ name = \"sum\", type = \"u32\", combine = \"sum\" }]\n\
                   in.count = { u32 = 100 }";
    let text = repeats_text(CHAIN_LOOP)
        .replace(clamped, &clamped.replace('8', "16384").replace("100", "16384"))
        .replace("max_count = 100000", "max_count = 2000000");
    let file = checked(&text);
    let cost = file.check().passes.iter().map(|pass| pass.cost).max().unwrap_or(0);
    assert!(
        cost > 32_768 && cost < 65_536,
        "the heaviest pass costs {cost} an agent"
    );
    let entry = model_entry(&text);
    let at = |agents: u32| [ParamValue::U32(agents), ParamValue::F32(40.0), ParamValue::F32(30.0)];
    let limits = roomy_limits();
    assert!(
        entry.shortfalls(&at(100_000), &limits).is_empty(),
        "a hundred thousand agents fit"
    );
    let refused = entry.shortfalls(&at(1_000_000), &limits);
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].contains("past the budget") && refused[0].contains("'node_agent_repeat_answers'"),
        "{refused:?}"
    );
    let Some(ctx) = baseline_device() else {
        return;
    };
    match entry.build(&at(1_000_000), None, Some(&ctx)) {
        Ok(_) => panic!("the build passed the budget"),
        Err(fault) => assert!(fault.to_string().contains("past the budget"), "{fault}"),
    }
}
