//! The graph starters compiled with a probe on every `bool` output of their tick passes: what the probes add to the
//! storage bindings, and a probed starter stepping as the unprobed one.

use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_core::params::ParamValue;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};
use henad_graph::GraphFile;
use henad_graph::document::{PassStage, World};
use henad_graph::types::ValueType;
use henad_graph::value::PortRef;
use henad_graph::wgsl::{AgentProgram, GridProgram, Probes};

use super::graph_grid_parity::{build_gpu, graph_cells, snapshot, starter_entry, step};

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
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping a_probed_starter_steps_as_the_unprobed_one: no adapter");
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
    let defaults: Vec<ParamValue> = plain
        .param_descriptors()
        .iter()
        .map(|param| param.kind().default_value())
        .collect();
    let mut probed_state = build_gpu(&probed, &defaults, Some(5), &ctx);
    let mut plain_state = build_gpu(&plain, &defaults, Some(5), &ctx);
    for tick in 0..40 {
        assert_same(&ctx, &mut *probed_state, &mut *plain_state, tick);
        step(&ctx, &mut *probed_state, 1);
        step(&ctx, &mut *plain_state, 1);
    }
    step(&ctx, &mut *probed_state, 64);
    step(&ctx, &mut *plain_state, 64);
    assert_same(&ctx, &mut *probed_state, &mut *plain_state, 104);
    assert_eq!(probed_state.fire_counts().counts.len(), probes.len());
    assert!(plain_state.fire_counts().is_empty());
}

/// Holds the two states equal in cells and stats at `tick`.
fn assert_same(ctx: &GpuContext, probed: &mut dyn GpuSimState, plain: &mut dyn GpuSimState, tick: u64) {
    assert!(
        graph_cells(probed) == graph_cells(plain),
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
