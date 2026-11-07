//! The fire-count channel of both GPU engines: the last step of each batch counts into the spec's fire range through
//! its counting block, and the counts read back with the stats, with the tick of the step they counted.

use std::borrow::Cow;
use std::sync::Arc;

use henad_core::authoring::model::binding::BindingKind;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{Domain, Geometry, PassCtx, PassId};
use henad_core::params::ParamValue;
use henad_core::view::{StatDescriptor, StatValue};

use crate::gpu::agent_engine::GpuAgentEngine;
use crate::gpu::agent_spec::{
    AgentBufferSpec, AgentHooks, AgentPassId, AgentPassSpec, AgentReduceSpec, AgentRun, GpuAgentSpec,
};
use crate::gpu::grid_engine::GpuGridEngine;
use crate::gpu::grid_spec::{GpuGridSpec, GridBufferSpec, GridHooks, GridPassCtx, GridPassId, GridPassSpec};
use crate::gpu::shader_spec::{ShaderBinding, ShaderSpec};
use crate::gpu::sides::SideTable;
use crate::gpu::sim_thread::GpuSimState;
use crate::gpu::{GpuContext, headless_context};
use crate::snapshot::FireCounts;

// Grid of the test grid model. One workgroup of 8 by 8 covers it.
const WIDTH: u32 = 8;
const HEIGHT: u32 = 4;
/// Number of cells of the test grid model, and of agents of the test agents model.
const ELEMENTS: u32 = WIDTH * HEIGHT;

/// Word of the counters the step counts into. Word 0 holds the reduce tally of the grid model.
const FIRE_WORD: usize = 1;

/// Sum of the seeds `1, 2, ..., ELEMENTS` of `count`.
const SEED_TOTAL: u32 = ELEMENTS * (ELEMENTS + 1) / 2;

/// Writes `count + 1` to the next side of `count`, and adds `count` to the fire word when the block counts.
const GRID_STEP: &str = r"
struct Params { width: u32, height: u32, counting: u32, _pad: u32 }

@group(0) @binding(0) var<storage, read> count_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> count_out: array<u32>;
@group(0) @binding(2) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(3) var<uniform> params: Params;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < params.width && id.y < params.height) {
        let i = id.y * params.width + id.x;
        count_out[i] = count_in[i] + 1u;
        if (params.counting != 0u) {
            atomicAdd(&counters[1], count_in[i]);
        }
    }
}
";

/// Writes nothing to its texture, which no test reads.
const GRID_DISPLAY: &str = r"
struct Dims { grid: vec2<u32>, tex: vec2<u32> }

@group(0) @binding(0) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(1) var<uniform> dims: Dims;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < dims.tex.x && id.y < dims.tex.y) {
        textureStore(output, vec2<i32>(id.xy), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    }
}
";

/// Sums `count` into the reduce tally, word 0.
const GRID_REDUCE: &str = r"
struct Dims { grid: vec2<u32>, tex: vec2<u32> }

@group(0) @binding(0) var<storage, read> count: array<u32>;
@group(0) @binding(1) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(2) var<uniform> dims: Dims;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < dims.grid.x && id.y < dims.grid.y) {
        atomicAdd(&counters[0], count[id.y * dims.grid.x + id.x]);
    }
}
";

/// Hooks of the test grid model, which seed `count` with `1, 2, ...` and write the counting flag into every block.
struct GridTestHooks;

impl GridHooks for GridTestHooks {
    fn dims(&self, _params: &[ParamValue]) -> (u32, u32) {
        (WIDTH, HEIGHT)
    }

    fn buffer_lens(&self, width: u32, height: u32) -> Vec<usize> {
        vec![(width * height) as usize]
    }

    fn step_dims(&self, width: u32, height: u32) -> (u32, u32) {
        (width, height)
    }

    fn seed_buffers(&self, _width: u32, _height: u32, _params: &[ParamValue], _seed: Option<u64>) -> Vec<Vec<u32>> {
        vec![(1..=ELEMENTS).collect()]
    }

    fn pass_params_bytes(&self, _pass: GridPassId, ctx: GridPassCtx, _params: &[ParamValue]) -> Vec<u8> {
        [ctx.width, ctx.height, u32::from(ctx.counting), 0]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    fn stats(&self, counts: &[u32], _ctx: GridPassCtx, _params: &[ParamValue]) -> Vec<StatValue> {
        vec![StatValue::Scalar(f64::from(counts[0]))]
    }
}

fn storage(name: &'static str, read_only: bool) -> ShaderBinding {
    ShaderBinding::new(name, BindingKind::Storage { read_only })
}

fn uniform(name: &'static str) -> ShaderBinding {
    ShaderBinding::new(name, BindingKind::Uniform)
}

/// Returns the test grid model: one step that counts the cells' values into word 1, and a reduce pass that sums them
/// into word 0.
fn grid_spec() -> Arc<GpuGridSpec> {
    let swapping = || SideTable::swapping(1);
    let step = GridPassSpec::new(
        "count_step",
        ShaderSpec::new(
            GRID_STEP,
            vec![
                storage("count_in", true),
                storage("count_out", false),
                storage("counters", false),
                uniform("params"),
            ],
        ),
        swapping(),
    );
    let display = GridPassSpec::new(
        "display",
        ShaderSpec::new(
            GRID_DISPLAY,
            vec![
                ShaderBinding::new("output", BindingKind::StorageTexture),
                uniform("dims"),
            ],
        ),
        swapping(),
    );
    let reduce = GridPassSpec::new(
        "reduce",
        ShaderSpec::new(
            GRID_REDUCE,
            vec![storage("count", true), storage("counters", false), uniform("dims")],
        ),
        swapping(),
    );
    let mut spec = GpuGridSpec::new(
        "fire_count_grid",
        8,
        vec![GridBufferSpec::new("count", true)],
        vec![step],
        display,
        reduce,
        Cow::Owned(vec![StatDescriptor::owned("Total".to_owned(), [1, 2, 3, 255])]),
        Box::new(GridTestHooks),
    );
    spec.counters = 2;
    spec.fire_counters = FIRE_WORD..FIRE_WORD + 1;
    Arc::new(spec)
}

/// Writes `count + 1` to the next side of `count`, and adds `count` to the fire word when the block counts.
const AGENT_STEP: &str = r"
struct Params { n: u32, counting: u32, _pad0: u32, _pad1: u32 }

@group(0) @binding(0) var<storage, read> count_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> count_out: array<u32>;
@group(0) @binding(2) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(3) var<uniform> params: Params;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    if (lid.x < params.n) {
        count_out[lid.x] = count_in[lid.x] + 1u;
        if (params.counting != 0u) {
            atomicAdd(&counters[1], count_in[lid.x]);
        }
    }
}
";

/// Sums `count` into the one partial of the single workgroup.
const AGENT_REDUCE: &str = r"
struct Params { n: u32, counting: u32, _pad0: u32, _pad1: u32 }

@group(0) @binding(0) var<storage, read> count: array<u32>;
@group(0) @binding(1) var<storage, read_write> partials: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    if (lid.x == 0u) {
        var total = 0.0;
        for (var i = 0u; i < params.n; i++) {
            total += f32(count[i]);
        }
        partials[0] = total;
    }
}
";

/// Hooks of the test agents model, which seed `count` with `1, 2, ...` and write the counting flag of the run into
/// every block.
struct AgentTestHooks;

impl AgentHooks for AgentTestHooks {
    fn dims(&self, _params: &[ParamValue]) -> (u32, Extent) {
        (ELEMENTS, Extent { w: 8.0, h: 8.0 })
    }

    fn buffer_lens(&self, geom: &Geometry) -> Vec<usize> {
        let agents = geom.num_agents as usize;
        vec![agents * 2, agents, agents]
    }

    fn seed_buffers(&self, geom: &Geometry, _params: &[ParamValue], _seed: Option<u64>) -> Vec<Vec<u8>> {
        let count: Vec<u8> = (1..=geom.num_agents).flat_map(u32::to_le_bytes).collect();
        vec![Vec::new(), Vec::new(), count]
    }

    fn index_cell_size(&self, _params: &[ParamValue]) -> f32 {
        1.0
    }

    fn pass_params_bytes(&self, _pass: PassId, _ctx: PassCtx<'_>, _params: &[ParamValue]) -> Vec<u8> {
        [ELEMENTS, 0, 0, 0].iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn run_params_bytes(
        &self,
        _pass: AgentPassId,
        _ctx: PassCtx<'_>,
        run: AgentRun,
        _params: &[ParamValue],
    ) -> Result<Vec<u8>, String> {
        Ok([ELEMENTS, u32::from(run.counting), 0, 0]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect())
    }

    fn stats(&self, sums: &[f32], _counters: &[u32], _geom: &Geometry) -> Vec<StatValue> {
        vec![StatValue::Scalar(f64::from(sums[0]))]
    }
}

/// Returns the test agents model: one step that counts the agents' values into word 1 of two counters.
fn agent_spec() -> Arc<GpuAgentSpec> {
    let buffers = vec![
        AgentBufferSpec::new("pos", false, true),
        AgentBufferSpec::new("color", false, true),
        AgentBufferSpec::new("count", true, false),
    ];
    let reduce = AgentReduceSpec::new(
        ShaderSpec::new(
            AGENT_REDUCE,
            vec![storage("count", true), storage("partials", false), uniform("params")],
        ),
        1,
        Domain::Agents,
        SideTable::swapping(buffers.len()),
    );
    let mut spec = GpuAgentSpec::new(
        "fire_count_agents",
        buffers,
        0,
        1,
        reduce,
        Cow::Owned(vec![StatDescriptor::owned("Total".to_owned(), [1, 2, 3, 255])]),
        Box::new(AgentTestHooks),
    );
    spec.steps = vec![AgentPassSpec::new(
        "count_step",
        ShaderSpec::new(
            AGENT_STEP,
            vec![
                storage("count_in", true),
                storage("count_out", false),
                storage("counters", false),
                uniform("params"),
            ],
        ),
        Domain::Agents,
        SideTable::swapping(spec.buffers.len()),
    )];
    spec.counters = 2;
    spec.fire_counters = FIRE_WORD..FIRE_WORD + 1;
    Arc::new(spec)
}

fn context() -> Option<GpuContext> {
    headless_context("fire_count_tests", wgpu::Features::empty())
}

/// Records `steps` steps and then the stats passes into one submission, as the runner does for a snapshot, and waits
/// for the readback.
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

/// Returns the fire count of the step that reached `tick`: the values every element held as the step began.
fn count_of_step(tick: u64) -> u32 {
    SEED_TOTAL + (tick as u32 - 1) * ELEMENTS
}

/// Returns the scalar of the one stat.
fn total(state: &dyn GpuSimState) -> f64 {
    match state.stats()[0].value() {
        StatValue::Scalar(value) => *value,
        other => panic!("not a scalar: {other:?}"),
    }
}

/// Steps `state` in calls of 1, 3 and 64 steps and checks after each that the counts hold the call's last step alone,
/// with the tick it reached, and that the stats read the same tick.
fn assert_last_step_counted(ctx: &GpuContext, state: &mut dyn GpuSimState, what: &str) {
    assert_eq!(
        state.fire_counts(),
        FireCounts::new(0, vec![0]),
        "{what}: no step counted yet"
    );
    let mut tick = 0;
    for calls in [1, 3, 64] {
        steps_then_stats(ctx, state, calls);
        tick += u64::from(calls);
        assert_eq!(
            state.fire_counts(),
            FireCounts::new(tick, vec![count_of_step(tick)]),
            "{what}: the counts after a call of {calls} steps"
        );
        assert_eq!(
            total(state),
            f64::from(SEED_TOTAL + tick as u32 * ELEMENTS),
            "{what}: the stats after a call of {calls} steps"
        );
    }
}

/// Each engine counts the last step of each `encode_steps` call, whatever its length, and never sums the steps of a
/// call.
#[test]
fn a_counted_step_reads_the_last_steps_count_in_calls_of_1_3_and_64() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_counted_step_reads_the_last_steps_count_in_calls_of_1_3_and_64: no adapter");
        return;
    };
    let mut grid = GpuGridEngine::new(&ctx, grid_spec(), &[], None);
    assert_last_step_counted(&ctx, &mut grid, "grid");
    let mut agents = GpuAgentEngine::new(&ctx, agent_spec(), &[], None);
    assert_last_step_counted(&ctx, &mut agents, "agents");
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// A grid's stats read the reduce tallies and the fire range in one readback, and a stats read with no step since
/// leaves the fire range as the last counted step left it.
#[test]
fn a_grid_reads_its_tallies_and_its_fire_range_in_one_readback() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_grid_reads_its_tallies_and_its_fire_range_in_one_readback: no adapter");
        return;
    };
    let mut grid = GpuGridEngine::new(&ctx, grid_spec(), &[], None);
    steps_then_stats(&ctx, &mut grid, 5);
    let counted = FireCounts::new(5, vec![count_of_step(5)]);
    assert_eq!(grid.fire_counts(), counted);
    assert_eq!(total(&grid), f64::from(SEED_TOTAL + 5 * ELEMENTS));
    for read in 1..=2 {
        steps_then_stats(&ctx, &mut grid, 0);
        assert_eq!(grid.fire_counts(), counted, "stats read {read} with no step");
        assert_eq!(
            total(&grid),
            f64::from(SEED_TOTAL + 5 * ELEMENTS),
            "stats read {read}: the tally is taken afresh"
        );
    }
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// A spec with no fire range counts nothing, and its counts are empty.
#[test]
fn a_spec_with_no_fire_range_has_empty_counts() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_spec_with_no_fire_range_has_empty_counts: no adapter");
        return;
    };
    let mut spec = Arc::try_unwrap(grid_spec()).unwrap_or_else(|_| unreachable!("the spec is not shared"));
    spec.fire_counters = 0..0;
    let mut grid = GpuGridEngine::new(&ctx, Arc::new(spec), &[], None);
    steps_then_stats(&ctx, &mut grid, 3);
    assert!(grid.fire_counts().is_empty());
    assert_eq!(total(&grid), f64::from(SEED_TOTAL + 3 * ELEMENTS));
}
