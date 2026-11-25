//! A field of agents that adds up its deposits, built through the model file and stepped on a device.
//!
//! Agent `i` sits at the centre of cell `(i % 40, (i / 40) % 30)` of a 40 by 30 world, so ten agents share each cell,
//! and each tick every agent deposits a random amount below 3 into its cell. The field adds up at the resolution
//! 2^-16, and its `finish` halves each cell.

use henad_core::params::ParamValue;
use henad_graph::GraphFile;
use henad_graph::document::PassStage;
use henad_graph::host::{HostEvaluator, HostValue, HostWorld, RunContext};

use super::support::{baseline_device, build_gpu, graph_buffer, model_entry, step};

/// Number of agents, ten to a cell.
const AGENTS: u32 = 12_000;
/// Columns and rows of the world, one cell a unit.
const SIDES: (u32, u32) = (40, 30);
/// Label of the field's buffer.
const FIELD_BUFFER: &str = "field_f_cells";

/// The model of every test: the world, a field `f` that adds up at 2^-16 and halves in its `finish`, an init pass that
/// places each agent at the centre of its cell, and a tick pass that deposits a draw below 3 into each agent's cell.
const MODEL: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_sum_field"

[graph]
topology = "agents"
library = 1

[[target]]
backend = "gpu"

[agents]
count = 12000
max_count = 100000
width = 40.0
height = 30.0

[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

[[field]]
id = "f"
type = "f32"
layers = ["value"]
static = false
combine = "sum"
resolution = 1.52587890625e-5
finish = "halve"

[[function]]
id = "halve"
kind = "pure"

[[function.input]]
port = "value"
type = "f32"

[[function.output]]
port = "value"
type = "f32"

[[function.node]]
id = "n201"
op = "fn.input"
config.port = "value"

[[function.node]]
id = "n202"
op = "math.mul"
in.a = "n201"
in.b = { f32 = 0.5 }

[[function.node]]
id = "n203"
op = "fn.output"
config.port = "value"
in.value = "n202"

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

[[pass]]
id = "lay"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n11"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n12"
op = "space.cell_of"
in.p = "n11"

[[pass.node]]
id = "n13"
op = "rand.float"
config.key = 7
in.max = { f32 = 3.0 }

[[pass.node]]
id = "n14"
op = "field.deposit"
config.field = "f"
in.layer = { layer = "f.value" }
in.at = "n12.cell"
in.value = "n13"
"#;

/// Returns the engine's parameters of the world.
fn world_params() -> [ParamValue; 3] {
    [
        ParamValue::U32(AGENTS),
        ParamValue::F32(SIDES.0 as f32),
        ParamValue::F32(SIDES.1 as f32),
    ]
}

/// Two runs of 20 ticks at one seed leave the same field bit for bit, and another seed leaves another field.
#[test]
fn a_sum_field_replays_exactly() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let entry = model_entry(MODEL);
    let run = |seed: u64| {
        let mut state = build_gpu(&entry, &world_params(), Some(seed), &ctx);
        step(&ctx, &mut *state, 20);
        graph_buffer(&entry, &*state, FIELD_BUFFER)
    };
    let first = run(3);
    assert_eq!(first.len(), (SIDES.0 * SIDES.1) as usize);
    assert!(
        first.iter().all(|&word| f32::from_bits(word) > 0.0),
        "every cell takes deposits"
    );
    assert_eq!(first, run(3), "one seed replays");
    assert_ne!(first, run(4), "another seed draws otherwise");
}

/// After one tick every cell of the field on the device is the host's word of deposits times the resolution, halved
/// by the `finish`, bit for bit.
#[test]
fn a_sum_field_merges_the_hosts_word() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let entry = model_entry(MODEL);
    let mut state = build_gpu(&entry, &world_params(), Some(3), &ctx);
    step(&ctx, &mut *state, 1);
    let device = graph_buffer(&entry, &*state, FIELD_BUFFER);

    let file = GraphFile::parse(MODEL).unwrap_or_else(|error| panic!("{error}"));
    let evaluator = HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{:?}", error.diagnostics()));
    let world = HostWorld::Agents {
        count: AGENTS,
        width: SIDES.0 as f32,
        height: SIDES.1 as f32,
    };
    let mut lanes = evaluator.init(world, &[], Some(3)).expect("the init runs");
    for pass in evaluator
        .document()
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::Tick)
    {
        let mut stream = 0;
        evaluator
            .run_pass(&pass.id, &mut lanes, &[], &RunContext::tick(Some(3), 0), &mut stream)
            .unwrap_or_else(|error| panic!("{}: {error}", pass.id));
    }
    let deposits = lanes.deposits("f.value").expect("the deposits of the field");
    for (cell, &found) in device.iter().enumerate() {
        let Some(HostValue::U32(word)) = deposits.get(cell) else {
            panic!("no word at cell {cell}");
        };
        assert!(
            word > 0 && word < 1 << 24,
            "cell {cell}: the word {word} merges exactly"
        );
        let expected = word as f32 / 65536.0 * 0.5;
        assert_eq!(
            found,
            expected.to_bits(),
            "cell {cell}: the device holds {}, and the host's word {word} gives {expected}",
            f32::from_bits(found)
        );
    }
}
