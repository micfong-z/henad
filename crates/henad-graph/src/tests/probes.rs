//! Probes of a compiled graph: the counts on the device against the host evaluator's, and the file's text and code
//! identity untouched by them.

use henad_compute::entry::ModelState;
use henad_compute::fault::FaultSink;
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::wgpu;
use henad_compute::snapshot::FireCounts;
use henad_core::params::ParamValue;

use super::support::test_device;
use crate::document::{PassEnabled, PassStage};
use crate::file::GraphFile;
use crate::host::{HostEvaluator, HostLanes, HostWorld, RunContext};
use crate::ids::NodeId;
use crate::value::PortRef;
use crate::wgsl::{AgentProgram, GridProgram, Probes, ShaderRole};

/// Node of the Game of Life starter that steps a cell with `grid.life_like`.
const LIFE_LIKE: &str = r#"[[pass.node]]
id = "n12"
op = "grid.life_like"
config.category = "life"
config.birth = [3]
config.survive = [2, 3]
config.dead = "Dead"
config.alive = "Alive"
in.cell = "n10"
in.count = "n11"
"#;

/// The rule B3/S23 as two transition rows: a Dead cell with 3 Alive neighbours becomes Alive, and an Alive cell with a
/// count other than 2 or 3 becomes Dead.
const TRANSITION_ROWS: &str = r#"[[pass.node]]
id = "n14"
op = "cmp.eq"
in.a = "n11"
in.b = { u32 = 3 }

[[pass.node]]
id = "n15"
op = "cmp.ne"
in.a = "n11"
in.b = { u32 = 2 }

[[pass.node]]
id = "n16"
op = "cmp.ne"
in.a = "n11"
in.b = { u32 = 3 }

[[pass.node]]
id = "n17"
op = "logic.and"
in.a = "n15"
in.b = "n16"

[[pass.node]]
id = "n12"
op = "cat.transition"
config.category = "life"
config.rules = [
  { id = "birth", from = "Dead", to = "Alive", phase = 0 },
  { id = "death", from = "Alive", to = "Dead", phase = 0 },
]
in.state = "n10"
in.rule.birth.when = "n14"
in.rule.death.when = "n17"
"#;

/// Returns the Game of Life starter with its step written as two `cat.transition` rows, as a sheet's lowering writes
/// it. The starter steps with `grid.life_like`, which has no rows to probe.
fn transition_life() -> GraphFile {
    let starter = crate::starters::starter("game_of_life.toml").expect("the starter");
    assert!(
        starter.text.contains(LIFE_LIKE),
        "the starter steps with grid.life_like"
    );
    let text = starter
        .text
        .replace(LIFE_LIKE, TRANSITION_ROWS)
        .replace("in.value = \"n12\"", "in.value = \"n12.next\"");
    GraphFile::parse(&text).unwrap_or_else(|error| panic!("{error}"))
}

/// Returns the reference to output `port` of node `number`.
fn output(number: u32, port: &str) -> PortRef {
    PortRef {
        node: NodeId::new(number).expect("a node number"),
        port: Some(port.to_owned()),
    }
}

/// Returns the probes on each transition row of [`transition_life`], and on the test of 3 Alive neighbours.
fn life_probes() -> Probes {
    Probes::new(vec![
        output(12, "fired.birth"),
        output(12, "fired.death"),
        output(14, "out"),
    ])
}

/// Returns a context over the test device.
fn context() -> Option<GpuContext> {
    let device = test_device()?;
    Some(GpuContext::new(
        device.device,
        device.queue,
        wgpu::TextureFormat::Rgba8Unorm,
        FaultSink::default(),
    ))
}

/// Records `steps` steps and the stats passes into one submission, as the runner does for a snapshot, and waits for
/// the readback.
fn steps_then_stats(ctx: &GpuContext, state: &mut dyn GpuSimState, steps: u32) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_steps(&mut encoder, steps, None);
    state.encode_stats_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    state.poll_stats_readback(&ctx.device, true);
}

/// Runs one step of the tick passes of `evaluator` on the host at `tick`, and returns the probes' counts.
fn host_step(
    evaluator: &HostEvaluator,
    lanes: &mut HostLanes,
    params: &[ParamValue],
    seed: Option<u64>,
    tick: u64,
    probes: &Probes,
) -> Vec<u32> {
    let document = evaluator.document();
    let values = evaluator.param_values(params);
    let mut counts = vec![0; probes.len()];
    let mut stream = 0;
    for (position, pass) in document.passes.iter().enumerate() {
        if pass.stage != PassStage::Tick || !evaluator.is_enabled(position, &values) {
            continue;
        }
        evaluator
            .run_pass_counting(
                &pass.id,
                lanes,
                params,
                &RunContext::tick(seed, tick as u32),
                &mut stream,
                probes,
                &mut counts,
            )
            .unwrap_or_else(|error| panic!("{error}"));
    }
    counts
}

/// Steps `state` and the host lanes one tick at a time for `ticks` ticks, then once in a call of 64 steps, and holds
/// the device's counts to the host's after each call.
#[expect(clippy::too_many_arguments, reason = "the two sides of the run and its declarations")]
fn assert_counts_follow_the_host(
    ctx: &GpuContext,
    state: &mut dyn GpuSimState,
    evaluator: &HostEvaluator,
    lanes: &mut HostLanes,
    params: &[ParamValue],
    seed: Option<u64>,
    probes: &Probes,
    ticks: u64,
) -> Vec<Vec<u32>> {
    let mut seen = Vec::new();
    for tick in 0..ticks {
        let host = host_step(evaluator, lanes, params, seed, tick, probes);
        steps_then_stats(ctx, state, 1);
        assert_eq!(
            state.fire_counts(),
            FireCounts::new(tick + 1, host.clone()),
            "the counts of the step that reached tick {}",
            tick + 1
        );
        seen.push(host);
    }
    let mut host = Vec::new();
    for tick in ticks..ticks + 64 {
        host = host_step(evaluator, lanes, params, seed, tick, probes);
    }
    steps_then_stats(ctx, state, 64);
    assert_eq!(
        state.fire_counts(),
        FireCounts::new(ticks + 64, host),
        "a call of 64 steps counts its last step"
    );
    seen
}

/// On the Game of Life starter written with transition rows, with a probe on each row and one on the test of 3 Alive
/// neighbours, the device's counts equal the host evaluator's every tick for 50 ticks, and after a call of 64 steps.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn a_probe_on_each_transition_row_counts_as_the_host_does() {
    let Some(ctx) = context() else {
        eprintln!("skipping a_probe_on_each_transition_row_counts_as_the_host_does: no adapter");
        return;
    };
    let file = transition_life();
    let probes = life_probes();
    let program = GridProgram::compile_with_probes(&file, &probes).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let spec = program.spec();
    assert_eq!(
        spec.fire_counters,
        2..5,
        "the counts follow the tallies of Dead and Alive"
    );
    let (width, height, seed) = (61, 37, Some(3));
    let own = [ParamValue::F32(0.35)];
    let params = [ParamValue::U32(width), ParamValue::U32(height), own[0].clone()];
    let mut state = match program.entry().build(&params, seed, Some(&ctx)) {
        Ok(ModelState::Gpu(state)) => state,
        Ok(ModelState::Cpu(_)) => panic!("the graph built on the CPU"),
        Err(fault) => panic!("the graph did not build: {fault}"),
    };
    let evaluator = HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{error}"));
    let world = HostWorld::Grid {
        width,
        height,
        boundary: henad_core::authoring::primitives::space::Boundary::Torus,
    };
    let mut lanes = evaluator
        .init(world, &own, seed)
        .unwrap_or_else(|error| panic!("{error}"));
    let seen = assert_counts_follow_the_host(&ctx, &mut *state, &evaluator, &mut lanes, &own, seed, &probes, 50);
    assert!(
        seen.iter().all(|counts| counts[0] > 0 && counts[1] > 0),
        "both rows fire on every tick of a busy grid: {seen:?}"
    );
    assert_eq!(
        state.read_buffer(0),
        lanes.column("state").map(|column| column.words().to_vec()),
        "the cells stay equal"
    );
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// Number of agents of [`a_probe_over_agents_counts_as_the_host_does`], four workgroups of 256 and part of a fifth.
const PROBED_AGENTS: u32 = 1100;

/// A probe over agents counts on the device as the host evaluator counts, every tick for 20 ticks and after a call of
/// 64 steps: the agents whose value, their index plus the tick, is below 600.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn a_probe_over_agents_counts_as_the_host_does() {
    let Some(ctx) = context() else {
        eprintln!("skipping a_probe_over_agents_counts_as_the_host_does: no adapter");
        return;
    };
    let mut text = super::case_files::agents_header(PROBED_AGENTS, 100, 100);
    text.push_str(
        "\n[[lane]]\nid = \"case_position\"\ndomain = \"agents\"\ntype = \"vec2f\"\nrole = \"position\"\n\n\
         [[lane]]\nid = \"value\"\ndomain = \"agents\"\ntype = \"u32\"\n\n\
         [[pass]]\nid = \"seed_values\"\nstage = \"init\"\nkind = \"map\"\ndomain = \"agents\"\n\n\
         [[pass.node]]\nid = \"n1\"\nop = \"ctx.index\"\n\n\
         [[pass.node]]\nid = \"n2\"\nop = \"lane.set\"\nconfig.lane = \"value\"\nin.value = \"n1\"\n\n\
         [[pass]]\nid = \"advance\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n\n\
         [[pass.node]]\nid = \"n10\"\nop = \"lane.get\"\nconfig.lane = \"value\"\n\n\
         [[pass.node]]\nid = \"n11\"\nop = \"math.add\"\nin.a = \"n10\"\nin.b = { u32 = 1 }\n\n\
         [[pass.node]]\nid = \"n12\"\nop = \"lane.set\"\nconfig.lane = \"value\"\nin.value = \"n11\"\n\n\
         [[pass.node]]\nid = \"n13\"\nop = \"cmp.lt\"\nin.a = \"n10\"\nin.b = { u32 = 600 }\n",
    );
    let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    let probes = Probes::new(vec![output(13, "out")]);
    let program = AgentProgram::compile_with_probes(&file, &probes).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let spec = program.spec();
    assert_eq!(spec.fire_counters.len(), 1);
    assert_eq!(spec.fire_counters.end, spec.counters, "the counts take the last words");
    let params = [
        ParamValue::U32(PROBED_AGENTS),
        ParamValue::F32(100.0),
        ParamValue::F32(100.0),
    ];
    let mut state = match program.entry().build(&params, None, Some(&ctx)) {
        Ok(ModelState::Gpu(state)) => state,
        Ok(ModelState::Cpu(_)) => panic!("the graph built on the CPU"),
        Err(fault) => panic!("the graph did not build: {fault}"),
    };
    let (evaluator, _, _) =
        crate::wgsl::agents::compile_parts(&file, &probes).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let world = HostWorld::Agents {
        count: PROBED_AGENTS,
        width: 100.0,
        height: 100.0,
    };
    let mut lanes = evaluator
        .init(world, &[], None)
        .unwrap_or_else(|error| panic!("{error}"));
    let seen = assert_counts_follow_the_host(&ctx, &mut *state, &evaluator, &mut lanes, &[], None, &probes, 20);
    let below = |tick: u32| 600u32.saturating_sub(tick);
    for (tick, counts) in seen.iter().enumerate() {
        assert_eq!(counts, &[below(tick as u32)], "the host's count at tick {tick}");
    }
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// Probes change the step shaders and the spec's fire range, and leave the file's text, its code identity and every
/// other shader alone. A graph compiled with no probes is the graph [`GridProgram::compile`] writes.
#[test]
fn probes_leave_the_code_identity_and_text_alone() {
    let file = transition_life();
    let text = file.to_toml();
    let plain = GridProgram::compile(&file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let none =
        GridProgram::compile_with_probes(&file, &Probes::default()).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let probed =
        GridProgram::compile_with_probes(&file, &life_probes()).unwrap_or_else(|problems| panic!("{problems:#?}"));
    assert_eq!(file.to_toml(), text, "compiling writes nothing to the file");

    let identity = |program: &GridProgram| program.entry().source().code_hash();
    assert_eq!(identity(&probed), identity(&plain), "the code identity");
    assert_eq!(identity(&plain), Some(file.code().hash()));

    let texts = |program: &GridProgram| -> Vec<(ShaderRole, String)> {
        program
            .shaders()
            .iter()
            .map(|shader| (shader.role().clone(), shader.text().to_owned()))
            .collect()
    };
    assert_eq!(texts(&none), texts(&plain), "no probes write the shaders of `compile`");
    assert!(plain.spec().fire_counters.is_empty());
    for ((role, plain_text), (_, probed_text)) in texts(&plain).iter().zip(&texts(&probed)) {
        if *role == ShaderRole::Step {
            assert!(probed_text.contains("henad_fire_tally"), "{probed_text}");
            assert!(!plain_text.contains("henad_fire_tally"));
        } else {
            assert_eq!(plain_text, probed_text, "{role:?} is unchanged");
        }
    }
    assert_eq!(probed.spec().fire_counters, 2..5);
}

/// A probe that names no output of a tick pass, or an output that holds no `bool`, is a problem of the compile.
#[test]
fn a_probe_on_no_bool_output_of_a_tick_pass_is_refused() {
    let file = transition_life();
    for (probe, wanted) in [
        (output(21, "Alive"), "names no output of a node in a tick pass"),
        (output(99, "out"), "names no output of a node in a tick pass"),
        (
            PortRef {
                node: NodeId::new(11).expect("a node number"),
                port: None,
            },
            "holds no bool",
        ),
    ] {
        let problems = GridProgram::compile_with_probes(&file, &Probes::new(vec![probe.clone()]))
            .expect_err("the probe is refused");
        assert!(
            problems.iter().any(|problem| problem.message.contains(wanted)),
            "{probe}: {problems:#?}"
        );
    }
}

/// A probe of a pass that `enabled = false` turns off counts nothing and builds no tally.
#[test]
fn a_probe_of_a_pass_that_never_runs_builds_no_tally() {
    let file = transition_life();
    let text = file.to_toml().replacen(
        "id = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\nenabled = true",
        "id = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\nenabled = false",
        1,
    );
    let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        file.document()
            .passes
            .iter()
            .any(|pass| pass.id == "step" && pass.enabled == PassEnabled::Literal(false))
    );
    let program =
        GridProgram::compile_with_probes(&file, &life_probes()).unwrap_or_else(|problems| panic!("{problems:#?}"));
    assert_eq!(program.spec().fire_counters.len(), 3, "each probe keeps its word");
    assert!(
        program
            .shaders()
            .iter()
            .all(|shader| !shader.text().contains("henad_fire_tally"))
    );
}

/// A probe on a node inside a region, which runs once per iteration, is a problem of the compile.
#[test]
fn a_probe_inside_a_region_is_refused() {
    let starter = crate::starters::starter("boids.toml").expect("the boids starter");
    let file = GraphFile::parse(starter.text).unwrap_or_else(|error| panic!("{error}"));
    let problems = AgentProgram::compile_with_probes(&file, &Probes::new(vec![output(26, "out")]))
        .expect_err("the probe is refused");
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("names a node inside a region")),
        "{problems:#?}"
    );
}
