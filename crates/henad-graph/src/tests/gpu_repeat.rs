//! The Repeat tests of the plan's "Finite loops" section on a device, through the compiler's own shaders.
//!
//! Most graphs are one of the host tests' bodies in a tick pass, since a model's init passes never run on the device.
//! The device runs the pass's step shader once over lanes that start at their declared initial values, and the host
//! evaluator runs the same pass with [`HostEvaluator::run_pass`] in the same context. One graph holds a Repeat in a
//! stats, a view and an action pass, the other map passes a Repeat can sit in.

use henad_core::authoring::primitives::rng::random_float;
use henad_core::params::ParamValue;

use super::host::{CARRIED, GRID, NESTED_DRAW, TWO_DRAWS, WORLD, chained};
use super::support::{ShaderRun, TestDevice, run_shader, test_device};
use super::wgsl::{LANE_SEED_BODY, POWI_BODY, repeat_nodes};
use crate::GraphFile;
use crate::draw::{PHASE_TICK, draw_bits, key_word};
use crate::host::{HostEvaluator, HostLanes, HostValue, HostWorld, RunContext};
use crate::wgsl::compile_shaders;

/// Returns the test device, or `None` with a note naming the skipped test `test` when this machine has none.
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn device_for(test: &str) -> Option<TestDevice> {
    let device = test_device();
    if device.is_none() {
        eprintln!("skipping {test}: no adapter");
    }
    device
}

/// Returns the file of [`GRID`] with one tick pass `step` holding `nodes`.
fn tick_file(nodes: &str) -> GraphFile {
    let text =
        format!("{GRID}\n[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\n{nodes}");
    GraphFile::parse(&text).unwrap_or_else(|error| panic!("the file reads: {error}\n{text}"))
}

/// Returns the lanes after one run of the pass `step` of `nodes` on the device at `tick`, one word per element.
fn on_device(device: &TestDevice, nodes: &str, seed: Option<u64>, tick: u32) -> Vec<Vec<u32>> {
    let file = tick_file(nodes);
    let report = file.check();
    let evaluator = HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{:?}", error.diagnostics()));
    let document = evaluator.document();
    let compiled =
        compile_shaders(document, &report, &evaluator, None).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let shader = compiled
        .shaders
        .iter()
        .find(|shader| shader.passes().iter().any(|pass| pass == "step"))
        .expect("the step shader");
    let HostWorld::Grid { width, height, .. } = WORLD else {
        unreachable!("the tests' world is a grid");
    };
    let lanes = HostLanes::new(document, WORLD).expect("the lanes");
    let mut buffers: Vec<Vec<u32>> = lanes.columns().iter().map(|column| column.words().to_vec()).collect();
    let mut labels: Vec<String> = lanes
        .columns()
        .iter()
        .map(|column| format!("lane_{}_cells", column.id()))
        .collect();
    if compiled.tick_buffer {
        buffers.push(vec![tick]);
        labels.push("henad_tick".to_owned());
    }
    let params = evaluator.param_values(&[]);
    let uniform = shader
        .uniform_bytes(&evaluator, WORLD, &params, &RunContext::tick(seed, tick))
        .unwrap_or_else(|error| panic!("{error}"));
    let result = run_shader(
        device,
        shader,
        &ShaderRun {
            width,
            height,
            buffers: &buffers,
            labels: &labels,
            uniform: &uniform,
        },
    );
    result.buffers[..lanes.columns().len()].to_vec()
}

/// Returns the lanes after one run of the pass `step` of `nodes` on the host at `tick`, one word per element.
fn on_host(nodes: &str, seed: Option<u64>, tick: u32) -> Vec<Vec<u32>> {
    let evaluator = HostEvaluator::new(&tick_file(nodes)).unwrap_or_else(|error| panic!("{:?}", error.diagnostics()));
    let mut lanes = HostLanes::new(evaluator.document(), WORLD).expect("the lanes");
    let no_params: [ParamValue; 0] = [];
    let mut stream = 0;
    evaluator
        .run_pass(
            "step",
            &mut lanes,
            &no_params,
            &RunContext::tick(seed, tick),
            &mut stream,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    lanes.columns().iter().map(|column| column.words().to_vec()).collect()
}

/// The integer half of a deterministic Repeat body of five iterations, a hash chain, equals five hand-chained copies of
/// the body bit for bit on the device, and both equal the host's run of the Repeat.
#[test]
fn a_repeat_body_equals_its_hand_chained_copies_on_the_device() {
    let Some(device) = device_for("a_repeat_body_equals_its_hand_chained_copies_on_the_device") else {
        return;
    };
    let looped = on_device(&device, CARRIED, None, 0);
    let unrolled = on_device(&device, &chained(), None, 0);
    let host = on_host(CARRIED, None, 0);
    assert_eq!(looped[0], unrolled[0], "the loop and its hand-chained copies");
    assert_eq!(looped[0], host[0], "the device and the host");
    assert_eq!(host, on_host(&chained(), None, 0), "the host's loop and copies");
    assert_ne!(looped[0][3], 3, "the body ran");
}

/// Both halves of the body of [`a_repeat_body_equals_its_hand_chained_copies_on_the_device`], the float half
/// `prev * 1.1 + k` included, equal bit for bit on the device and on the host.
///
/// Note that this fails on an Apple M4 Pro. wgpu compiles Metal shaders with fast math on, and the device contracts,
/// reassociates and folds the float half differently in the loop and in the copies, whose `k` are literals. The integer
/// half holds. Compiled with `MTLMathMode.safe`, the same shaders equal each other and the host. Records #66 and #67
/// hold the probes.
#[test]
#[ignore = "fails on Metal, which compiles with fast math; records #66 and #67"]
fn a_float_repeat_body_equals_its_hand_chained_copies_on_the_device() {
    let Some(device) = device_for("a_float_repeat_body_equals_its_hand_chained_copies_on_the_device") else {
        return;
    };
    let looped = on_device(&device, CARRIED, None, 0);
    assert_eq!(
        looped,
        on_device(&device, &chained(), None, 0),
        "the loop and its hand-chained copies"
    );
    assert_eq!(looped, on_host(CARRIED, None, 0), "the device and the host");
}

/// The float half of [`CARRIED`] replays on one device: two runs of the loop leave the same words, as do two runs of
/// its hand-chained copies.
#[test]
fn a_float_repeat_body_replays_on_the_device() {
    let Some(device) = device_for("a_float_repeat_body_replays_on_the_device") else {
        return;
    };
    for nodes in [CARRIED.to_owned(), chained()] {
        let first = on_device(&device, &nodes, None, 0);
        assert_eq!(
            first,
            on_device(&device, &nodes, None, 0),
            "one device replays:\n{nodes}"
        );
        assert_ne!(first[1][3], 3.0f32.to_bits(), "the body ran:\n{nodes}");
    }
}

/// The two iterations of one draw in a Repeat differ at every cell on the device, two runs at one seed are equal, and
/// each draw is the counter-based draw of its cell, tick and iteration, as the host computes it.
#[test]
fn two_iterations_of_one_draw_differ_on_the_device_and_one_seed_replays() {
    let Some(device) = device_for("two_iterations_of_one_draw_differ_on_the_device_and_one_seed_replays") else {
        return;
    };
    for (seed, tick) in [(Some(11), 0), (None, 7)] {
        let first = on_device(&device, TWO_DRAWS, seed, tick);
        let again = on_device(&device, TWO_DRAWS, seed, tick);
        assert_eq!(first, again, "one seed replays");
        assert!(first[0].iter().all(|&differ| differ == 1), "the iterations differ");
        assert_eq!(first, on_host(TWO_DRAWS, seed, tick), "the device and the host");
        let key = key_word(4242, PHASE_TICK, seed);
        for (element, &word) in first[1].iter().enumerate() {
            let bits = draw_bits(key, element as u32, tick, Some(0), 0);
            assert_eq!(
                HostValue::F32(f32::from_bits(word)),
                HostValue::F32(random_float(bits, 1.0)),
                "the draw of cell {element} at tick {tick}"
            );
        }
    }
    let other_tick = on_device(&device, TWO_DRAWS, Some(11), 1);
    assert_ne!(
        on_device(&device, TWO_DRAWS, Some(11), 0)[1],
        other_tick[1],
        "two ticks draw apart"
    );
}

/// A draw two Repeats deep reads the mixed-radix iteration word on the device, as on the host.
#[test]
fn nested_iterations_combine_in_mixed_radix_on_the_device() {
    let Some(device) = device_for("nested_iterations_combine_in_mixed_radix_on_the_device") else {
        return;
    };
    let lanes = on_device(&device, NESTED_DRAW, Some(5), 2);
    assert_eq!(lanes, on_host(NESTED_DRAW, Some(5), 2), "the device and the host");
    let key = key_word(77, PHASE_TICK, Some(5));
    for (element, &word) in lanes[1].iter().enumerate() {
        let bits = draw_bits(key, element as u32, 2, Some(4 + 2), 0);
        assert_eq!(
            f32::from_bits(word).to_bits(),
            random_float(bits, 1.0).to_bits(),
            "the draw of cell {element}"
        );
    }
}

/// Tables appended to [`GRID`]: a palette of nine colours, three statistics, an action `poke`, and four passes that
/// each hold a Repeat whose count is the cell's `x`.
///
/// The tick pass keeps `x`. The stats pass `counts` sums `k`, carries a hash chain started from the cell's index and
/// counts its iterations, then counts the cells whose sum is 6, whose chain ends odd and whose count was clamped to
/// `max`. The view pass `paint` counts the even iterations, with `when` false at `x = 3`, and colours the cell by that
/// count. The action pass `poke` carries `3 * prev + k` from the cell's index and writes it to `x`.
const CONTEXTS: &str = r#"
[[palette]]
id = "ramp"
colors = [[0, 0, 0], [30, 0, 0], [60, 0, 0], [90, 0, 0], [120, 0, 0], [150, 0, 0], [180, 0, 0], [210, 0, 0], [240, 0, 0]]

[[stat]]
label = "SumIsSix"
from = "n28"
color = [242, 166, 59, 255]

[[stat]]
label = "ChainOdd"
from = "n31"
color = [242, 166, 59, 255]

[[stat]]
label = "Clamped"
from = "n33"
color = [242, 166, 59, 255]

[[action]]
id = "poke"
label = "Poke"
runs = []

[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "x"
in.value = "n1"

[[pass]]
id = "counts"
stage = "stats"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n20"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n21"
op = "region.repeat"
config.max = 8
config.acc = [
  { name = "s", type = "u32", combine = "sum" },
  { name = "w", type = "u32", combine = "carry" },
  { name = "n", type = "u32", combine = "count" },
]
in.count = "n20"
in.init.w = "n22"

[[pass.node]]
id = "n22"
op = "ctx.index"

[[pass.node]]
id = "n23"
op = "region.put"
config.acc = "s"
in.value = "n21.k"
region = "n21"

[[pass.node]]
id = "n24"
op = "bits.xor"
in.a = "n21.prev.w"
in.b = "n21.k"
region = "n21"

[[pass.node]]
id = "n25"
op = "rng.hash"
in.x = "n24"
region = "n21"

[[pass.node]]
id = "n26"
op = "region.put"
config.acc = "w"
in.value = "n25"
region = "n21"

[[pass.node]]
id = "n34"
op = "region.put"
config.acc = "n"
region = "n21"

[[pass.node]]
id = "n27"
op = "cmp.eq"
in.a = "n21.s"
in.b = { u32 = 6 }

[[pass.node]]
id = "n28"
op = "stat.count_where"
in.pred = "n27"

[[pass.node]]
id = "n29"
op = "bits.and"
in.a = "n21.w"
in.b = { u32 = 1 }

[[pass.node]]
id = "n30"
op = "cmp.eq"
in.a = "n29"
in.b = { u32 = 1 }

[[pass.node]]
id = "n31"
op = "stat.count_where"
in.pred = "n30"

[[pass.node]]
id = "n32"
op = "cmp.eq"
in.a = "n21.n"
in.b = { u32 = 8 }

[[pass.node]]
id = "n33"
op = "stat.count_where"
in.pred = "n32"

[[pass]]
id = "paint"
stage = "view"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n40"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n41"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "c", type = "u32", combine = "count" }]
in.count = "n40"
in.when = "n42"

[[pass.node]]
id = "n42"
op = "cmp.ne"
in.a = "n40"
in.b = { u32 = 3 }

[[pass.node]]
id = "n43"
op = "bits.and"
in.a = "n41.k"
in.b = { u32 = 1 }
region = "n41"

[[pass.node]]
id = "n44"
op = "cmp.eq"
in.a = "n43"
in.b = { u32 = 0 }
region = "n41"

[[pass.node]]
id = "n45"
op = "region.put"
config.acc = "c"
in.when = "n44"
region = "n41"

[[pass.node]]
id = "n46"
op = "view.color"
config.palette = "ramp"
in.index = "n41.c"

[[pass]]
id = "poke_cells"
stage = "action"
action = "poke"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n50"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n51"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "v", type = "u32", combine = "carry" }]
in.count = "n50"
in.init.v = "n52"

[[pass.node]]
id = "n52"
op = "ctx.index"

[[pass.node]]
id = "n53"
op = "math.mul"
in.a = "n51.prev.v"
in.b = { u32 = 3 }
region = "n51"

[[pass.node]]
id = "n54"
op = "math.add"
in.a = "n53"
in.b = "n51.k"
region = "n51"

[[pass.node]]
id = "n55"
op = "region.put"
config.acc = "v"
in.value = "n54"
region = "n51"

[[pass.node]]
id = "n56"
op = "lane.set"
config.lane = "x"
in.value = "n51.v"
"#;

/// Returns the `x` of cell `element` in the lanes of [`a_repeat_runs_in_stats_view_and_action_passes_on_the_device`],
/// past `max = 8` at three cells in eleven.
fn context_x(element: usize) -> u32 {
    (element % 11) as u32
}

/// A Repeat whose count varies per cell runs in a stats, a view and an action pass on the device, and each pass
/// leaves what the host's run of it leaves: the three statistics, the colour of every cell and the lane `x`.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the three passes and their known values read in order"
)]
fn a_repeat_runs_in_stats_view_and_action_passes_on_the_device() {
    let Some(device) = device_for("a_repeat_runs_in_stats_view_and_action_passes_on_the_device") else {
        return;
    };
    let text = format!("{GRID}\n{CONTEXTS}");
    let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("the file reads: {error}"));
    let report = file.check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    let evaluator = HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{:?}", error.diagnostics()));
    let document = evaluator.document();
    let compiled =
        compile_shaders(document, &report, &evaluator, None).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let HostWorld::Grid { width, height, .. } = WORLD else {
        unreachable!("the tests' world is a grid");
    };
    let mut start = HostLanes::new(document, WORLD).expect("the lanes");
    let column = start.column_mut("x").expect("the lane x");
    for element in 0..(width * height) as usize {
        assert!(column.set(element, HostValue::U32(context_x(element))), "an x");
    }
    let params = evaluator.param_values(&[]);
    let no_params: [ParamValue; 0] = [];
    let run = |pass: &str, context: &RunContext| {
        let shader = compiled
            .shaders
            .iter()
            .find(|shader| shader.passes().iter().any(|known| known == pass))
            .unwrap_or_else(|| panic!("no shader runs `{pass}`"));
        let mut buffers: Vec<Vec<u32>> = start.columns().iter().map(|column| column.words().to_vec()).collect();
        let mut labels: Vec<String> = start
            .columns()
            .iter()
            .map(|column| format!("lane_{}_cells", column.id()))
            .collect();
        if compiled.tick_buffer {
            buffers.push(vec![context.tick]);
            labels.push("henad_tick".to_owned());
        }
        let uniform = shader
            .uniform_bytes(&evaluator, WORLD, &params, context)
            .unwrap_or_else(|error| panic!("{error}"));
        let result = run_shader(
            &device,
            shader,
            &ShaderRun {
                width,
                height,
                buffers: &buffers,
                labels: &labels,
                uniform: &uniform,
            },
        );
        let mut lanes = start.clone();
        let outcome = evaluator
            .run_pass(pass, &mut lanes, &no_params, context, &mut 0)
            .unwrap_or_else(|error| panic!("{error}"));
        (shader, result, lanes, outcome)
    };

    // The stats pass: the device counts the tallies, and the host finishes the statistics from them.
    let tick = RunContext::tick(None, 0);
    let (shader, result, _, outcome) = run("counts", &tick);
    let mut first = 0;
    for pass in shader.passes().iter().take_while(|pass| *pass != "counts") {
        first += evaluator.program(pass).expect("a stats pass").1.tallies;
    }
    let (_, program) = evaluator.program("counts").expect("the stats pass");
    let tallies: Vec<u64> = result.counters[first..first + program.tallies]
        .iter()
        .map(|&count| u64::from(count))
        .collect();
    let frame = crate::host::run::finish(program, &params, &tick, WORLD, Some(&tallies))
        .unwrap_or_else(|error| panic!("{error}"));
    let stat = |node: u32| -> HostValue {
        let mut ports = outcome.values[&crate::ids::NodeId::new(node).expect("an id")].iter();
        let (port, host) = ports.next().expect("a statistic");
        assert!(ports.next().is_none(), "one output of n{node}");
        let slot = program.outputs[&(crate::ids::NodeId::new(node).expect("an id"), port.clone())];
        assert_eq!(
            frame[slot],
            *host,
            "n{node} on the device and the host\n{}",
            shader.text()
        );
        *host
    };
    let cells = (0..(width * height) as usize).map(context_x);
    let count = |pred: &dyn Fn(u32) -> bool| HostValue::U64(cells.clone().filter(|&x| pred(x)).count() as u64);
    assert_eq!(stat(28), count(&|x| x == 4), "the sum of k is 6 at a count of 4");
    assert_eq!(stat(33), count(&|x| x >= 8), "a count of 8 or more is clamped to 8");
    let odd = stat(31);
    assert!(
        odd != HostValue::U64(0) && odd != HostValue::U64(u64::from(width * height)),
        "the chains end odd at some cells and not at others: {odd:?}"
    );

    // The view pass: the colour of each cell is its count of even iterations, none where `when` is false.
    let (_, result, _, outcome) = run("paint", &tick);
    let colors = outcome.colors.expect("the view pass colours");
    let found: Vec<Option<u32>> = result
        .texels
        .iter()
        .map(|texel| super::gpu_cases::color_index(document, "ramp", *texel))
        .collect();
    assert_eq!(
        found,
        colors.iter().map(|&index| Some(index)).collect::<Vec<_>>(),
        "colours"
    );
    let expected: Vec<u32> = cells
        .clone()
        .map(|x| if x == 3 { 0 } else { x.min(8).div_ceil(2) })
        .collect();
    assert_eq!(colors, expected, "each cell's count of even iterations");

    // The action pass: `x` holds the carry after `min(x, 8)` steps of `3 * prev + k` from the cell's index.
    let (_, result, lanes, _) = run("poke_cells", &RunContext::action(None, 0, 1, 0));
    let position = document
        .lanes
        .iter()
        .position(|lane| lane.id == "x")
        .expect("the lane x");
    assert_eq!(result.buffers[position], lanes.columns()[position].words(), "x");
    let expected: Vec<u32> = cells
        .enumerate()
        .map(|(element, x)| (0..x.min(8)).fold(element as u32, |prev, k| prev.wrapping_mul(3).wrapping_add(k)))
        .collect();
    assert_eq!(lanes.columns()[position].words(), expected, "each cell's carry");
}

/// A Repeat body whose lane seed and powi base are the same in every iteration runs on the device, reading both from
/// the uniform block, and equals the host.
#[test]
fn uniform_values_of_a_repeat_body_run_on_the_device() {
    let Some(device) = device_for("uniform_values_of_a_repeat_body_run_on_the_device") else {
        return;
    };
    for (body, seed) in [(LANE_SEED_BODY, Some(3)), (LANE_SEED_BODY, None), (POWI_BODY, None)] {
        let nodes = repeat_nodes(body);
        let lanes = on_device(&device, &nodes, seed, 0);
        assert_eq!(lanes, on_host(&nodes, seed, 0), "the device and the host:\n{nodes}");
        assert_ne!(lanes[0][0], 0, "the body ran:\n{nodes}");
    }
    // A base of 0.5 * 3 raised to the powers 0, 1 and 2, the largest times 1000.
    assert_eq!(on_device(&device, &repeat_nodes(POWI_BODY), None, 0)[0][0], 2250);
}
