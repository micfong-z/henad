//! The graph starters compiled with a probe on every `bool` output of their tick passes: what the probes add to the
//! storage bindings, a probed starter stepping as the unprobed one, and the barriers of every probed shader, the
//! sheets' included, in uniform control flow.

use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_explore::model_file::sheet_probes;
use henad_graph::GraphFile;
use henad_graph::document::{PassStage, World};
use henad_graph::sheet::SheetFile;
use henad_graph::types::ValueType;
use henad_graph::value::PortRef;
use henad_graph::wgsl::{AgentProgram, FaultNames, GeneratedShader, GridProgram, Probes, ShaderRole};

use super::lanes::GraphLanes;
use super::support::{baseline_device, build_gpu, fixture_text, params_for, snapshot, starter_entry, step};

/// Kinds that normalisation expands into copies, whose outputs no compiled pass computes under their own names.
const EXPANDED: [&str; 2] = ["math.expr", "fn.call"];

/// Returns the probes on every `bool` output of a node in a tick pass of `file`, outside every region and of no kind
/// in [`EXPANDED`], in pass and node order.
fn every_bool_output(file: &GraphFile) -> Probes {
    let report = file.check();
    let outputs = file
        .document()
        .passes
        .iter()
        .zip(&report.passes)
        .filter(|(pass, _)| pass.stage == PassStage::Tick)
        .flat_map(|(pass, checked)| {
            checked.outputs.iter().filter(|(node, _)| {
                pass.body.nodes.iter().any(|written| {
                    written.id == **node && written.region.is_none() && !EXPANDED.contains(&&*written.op)
                })
            })
        })
        .flat_map(|(node, ports)| {
            ports
                .iter()
                .filter(|(_, port)| port.value_type == Some(ValueType::Bool))
                .map(|(port, _)| PortRef {
                    node: *node,
                    port: Some(port.clone()),
                })
        })
        .collect();
    Probes::new(outputs)
}

/// Returns the most storage buffers a pass of `file` binds, compiled with `probes`.
fn storage_bindings(file: &GraphFile, probes: &Probes) -> u32 {
    let name = &file.document().id;
    match file.document().world {
        World::Grid(_) => GridProgram::compile_with_probes(file, probes)
            .unwrap_or_else(|problems| panic!("{name}: {problems:#?}"))
            .spec()
            .max_storage_bindings(),
        World::Agents(_) => AgentProgram::compile_with_probes(file, probes)
            .unwrap_or_else(|problems| panic!("{name}: {problems:#?}"))
            .spec()
            .max_storage_bindings(),
        _ => panic!("{name}: a world no compiler compiles"),
    }
}

/// A probe adds the `counters` binding to a step that binds none, and no starter, probed on every `bool` output of
/// its tick passes outside a region, binds more storage buffers than the WebGPU default of 8.
#[test]
fn probes_add_at_most_one_storage_binding_and_no_starter_passes_eight() {
    let starters = henad_graph::starters::starters()
        .iter()
        .chain(henad_graph::starters::agent_starters());
    let mut probed_any = false;
    for starter in starters {
        let file = GraphFile::parse(starter.text).unwrap_or_else(|error| panic!("{}: {error}", starter.file_name));
        let probes = every_bool_output(&file);
        probed_any |= !probes.is_empty();
        let plain = storage_bindings(&file, &Probes::default());
        let probed = storage_bindings(&file, &probes);
        assert!(
            probed == plain || probed == plain + 1,
            "{}: {plain} storage bindings, {probed} with {} probes",
            starter.file_name,
            probes.len()
        );
        assert!(
            probed <= 8,
            "{}: {probed} storage bindings with {} probes",
            starter.file_name,
            probes.len()
        );
    }
    assert!(probed_any, "some starter has a bool output in a tick pass");
}

/// The SIR starter probed on every `bool` output of its tick pass equals the starter with no probes in cells and
/// stats every tick, so the counting step changes no state.
#[test]
fn a_probed_starter_steps_as_the_unprobed_one() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let starter = henad_graph::starters::starter("sir.toml").expect("the SIR starter");
    let file = GraphFile::parse(starter.text).unwrap_or_else(|error| panic!("{error}"));
    let probes = every_bool_output(&file);
    assert!(!probes.is_empty(), "the SIR starter has a bool output in its tick pass");
    let probed = GridProgram::compile_with_probes(&file, &probes)
        .unwrap_or_else(|problems| panic!("{problems:#?}"))
        .entry();
    let plain = starter_entry("sir.toml");
    let lanes = GraphLanes::of_starter("sir.toml");
    let defaults = params_for(&plain, &[]);
    let mut probed_state = build_gpu(&probed, &defaults, Some(5), &ctx);
    let mut plain_state = build_gpu(&plain, &defaults, Some(5), &ctx);
    for tick in 0..40 {
        assert_same(&ctx, &lanes, &mut *probed_state, &mut *plain_state, tick);
        step(&ctx, &mut *probed_state, 1);
        step(&ctx, &mut *plain_state, 1);
    }
    step(&ctx, &mut *probed_state, 64);
    step(&ctx, &mut *plain_state, 64);
    assert_same(&ctx, &lanes, &mut *probed_state, &mut *plain_state, 104);
    assert_eq!(probed_state.fire_counts().counts.len(), probes.len());
    assert!(plain_state.fire_counts().is_empty());
}

/// Holds the two states equal in cells and stats at `tick`.
fn assert_same(
    ctx: &GpuContext,
    lanes: &GraphLanes,
    probed: &mut dyn GpuSimState,
    plain: &mut dyn GpuSimState,
    tick: u64,
) {
    assert!(
        lanes.read(probed, "health") == lanes.read(plain, "health"),
        "the cells differ at tick {tick}"
    );
    snapshot(ctx, probed);
    snapshot(ctx, plain);
    assert_eq!(
        format!("{:?}", probed.stats()),
        format!("{:?}", plain.stats()),
        "the stats differ at tick {tick}"
    );
}

/// Returns the shaders of `file` compiled with `probes`, and with `faults` naming its radius folds when it is a
/// sheet's.
fn probed_shaders(file: &GraphFile, probes: &Probes, faults: Option<&FaultNames>) -> Vec<GeneratedShader> {
    let name = &file.document().id;
    match file.document().world {
        World::Grid(_) => GridProgram::compile_with_probes(file, probes)
            .unwrap_or_else(|problems| panic!("{name}: {problems:#?}"))
            .shaders()
            .to_vec(),
        World::Agents(_) => match faults {
            Some(faults) => AgentProgram::compile_with_probes_and_fault_names(file, probes, faults),
            None => AgentProgram::compile_with_probes(file, probes),
        }
        .unwrap_or_else(|problems| panic!("{name}: {problems:#?}"))
        .shaders()
        .to_vec(),
        _ => panic!("{name}: a world no compiler compiles"),
    }
}

/// Opening line of the one block other than `main` that may hold a workgroup barrier. Its condition reads a uniform,
/// so the barrier stays in uniform control flow.
const COUNTING: &str = "if (params.henad_count_fires != 0u) {";

/// Holds that `main` of `shader`, a shader of `name`, never returns early, and that each `workgroupBarrier()` sits at
/// the top level of `main` or directly inside a [`COUNTING`] block there.
fn assert_uniform_barriers(name: &str, shader: &GeneratedShader) {
    let text = shader.text();
    let main = &text[text.find("fn main(").expect("a main")..];
    if *shader.role() != ShaderRole::Display {
        assert!(
            !main.contains("return"),
            "{name} {} returns early:\n{text}",
            shader.label()
        );
    }
    let mut open: Vec<&str> = Vec::new();
    for line in main.lines() {
        if line.contains("workgroupBarrier()") {
            let uniform = match open.as_slice() {
                [_] => true,
                [_, block] => block.trim() == COUNTING,
                _ => false,
            };
            assert!(uniform, "{name} {}: a barrier inside {open:?}\n{text}", shader.label());
        }
        for character in line.chars() {
            match character {
                '{' => open.push(line),
                '}' => {
                    open.pop();
                }
                _ => {}
            }
        }
    }
}

/// Every probed shader keeps its barriers in uniform control flow: the graph starters probed on every `bool` output of
/// their tick passes, and every sheet starter and grid sheet fixture compiled as a model file compiles it, with the
/// probes of its change lines. naga accepts a barrier after an early return, so only this check holds the rule.
#[test]
fn every_probed_shader_keeps_its_barriers_in_uniform_control_flow() {
    let (mut barriers, mut counting) = (0, 0);
    let mut check = |name: &str, shaders: &[GeneratedShader]| {
        for shader in shaders {
            assert_uniform_barriers(name, shader);
            barriers += shader.text().matches("workgroupBarrier()").count();
            counting += shader.text().matches(COUNTING).count();
        }
    };
    let starters = henad_graph::starters::starters()
        .iter()
        .chain(henad_graph::starters::agent_starters());
    for starter in starters {
        let file = GraphFile::parse(starter.text).unwrap_or_else(|error| panic!("{}: {error}", starter.file_name));
        check(
            starter.file_name,
            &probed_shaders(&file, &every_bool_output(&file), None),
        );
    }
    let fixtures = ["sir_chance.toml", "sird.toml"].map(|name| (name, fixture_text(&format!("sheets/{name}"))));
    let sheets = henad_graph::sheet::starters()
        .iter()
        .map(|starter| (starter.file_name, starter.text.to_owned()))
        .chain(fixtures);
    for (name, text) in sheets {
        let file = SheetFile::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        check(
            name,
            &probed_shaders(file.lowered(), sheet_probes(&file), Some(file.fault_names())),
        );
    }
    assert!(
        barriers > 0 && counting > 0,
        "the probed shaders hold barriers, some inside a counting block"
    );
}
