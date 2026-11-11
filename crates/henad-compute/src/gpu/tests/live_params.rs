//! Live parameters of both GPU engines: a live edit writes the uniform blocks again between steps, so the tick and the
//! buffers stay and later steps read the new value, and a reload-only parameter is rejected.

use std::sync::Arc;

use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{Geometry, PassCtx, PassId};
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::view::StatValue;

use crate::gpu::agent_engine::GpuAgentEngine;
use crate::gpu::agent_spec::{AgentHooks, AgentPassId, AgentRun, GpuAgentSpec};
use crate::gpu::grid_engine::GpuGridEngine;
use crate::gpu::grid_spec::{GpuGridSpec, GridHooks, GridPassCtx, GridPassId};
use crate::gpu::shader_spec::ShaderSpec;
use crate::gpu::sim_thread::GpuSimState;
use crate::gpu::tests::support::{count_agent_spec, count_grid_spec, scalar_stat, steps_then_stats, storage, uniform};
use crate::gpu::{GpuContext, headless_context};

// Grid of the test grid model. One workgroup of 8 by 8 covers it.
const WIDTH: u32 = 8;
const HEIGHT: u32 = 4;
/// Number of cells of the test grid model, and of agents of the test agents model.
const ELEMENTS: u32 = WIDTH * HEIGHT;

// Parameters of both test models: the amount each step adds, live, and the start of every element, reload-only.
const AMOUNT: usize = 0;
const START: usize = 1;

/// Amount at which the agents model's hooks cannot write a block.
const REFUSED_AMOUNT: u32 = 999;

/// Adds the amount to `count`, writing the next side, and to the fire word when the block counts.
const GRID_STEP: &str = r"
struct Params { width: u32, height: u32, counting: u32, amount: u32 }

@group(0) @binding(0) var<storage, read> count_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> count_out: array<u32>;
@group(0) @binding(2) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(3) var<uniform> params: Params;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < params.width && id.y < params.height) {
        let i = id.y * params.width + id.x;
        count_out[i] = count_in[i] + params.amount;
        if (params.counting != 0u) {
            atomicAdd(&counters[1], params.amount);
        }
    }
}
";

/// Returns the `u32` parameter at `index` of `params`, 0 when it is missing or of another type.
fn word(params: &[ParamValue], index: usize) -> u32 {
    match params.get(index) {
        Some(ParamValue::U32(value)) => *value,
        _ => 0,
    }
}

/// Hooks of the test grid model, which seed every cell with the start and write the amount into the step's block.
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

    fn seed_buffers(&self, _width: u32, _height: u32, params: &[ParamValue], _seed: Option<u64>) -> Vec<Vec<u32>> {
        vec![vec![word(params, START); ELEMENTS as usize]]
    }

    fn pass_params_bytes(&self, _pass: GridPassId, ctx: GridPassCtx, params: &[ParamValue]) -> Vec<u8> {
        [ctx.width, ctx.height, u32::from(ctx.counting), word(params, AMOUNT)]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    fn stats(&self, counts: &[u32], _ctx: GridPassCtx, _params: &[ParamValue]) -> Vec<StatValue> {
        vec![StatValue::Scalar(f64::from(counts[0]))]
    }
}

/// Returns the test grid model: one step that adds the amount to every cell and to the fire word, and a reduce pass
/// that sums the cells into word 0.
fn grid_spec() -> Arc<GpuGridSpec> {
    count_grid_spec("live_param_grid", "add_step", GRID_STEP, Box::new(GridTestHooks))
}

/// Adds the amount to `count`, writing the next side.
const AGENT_STEP: &str = r"
struct Params { n: u32, amount: u32, _pad0: u32, _pad1: u32 }

@group(0) @binding(0) var<storage, read> count_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> count_out: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    if (lid.x < params.n) {
        count_out[lid.x] = count_in[lid.x] + params.amount;
    }
}
";

/// Hooks of the test agents model, which seed every agent with the start and write the amount into every block, and
/// fail at [`REFUSED_AMOUNT`].
struct AgentTestHooks;

impl AgentHooks for AgentTestHooks {
    fn dims(&self, _params: &[ParamValue]) -> (u32, Extent) {
        (ELEMENTS, Extent { w: 8.0, h: 8.0 })
    }

    fn buffer_lens(&self, geom: &Geometry) -> Vec<usize> {
        let agents = geom.num_agents as usize;
        vec![agents * 2, agents, agents]
    }

    fn seed_buffers(&self, geom: &Geometry, params: &[ParamValue], _seed: Option<u64>) -> Vec<Vec<u8>> {
        let start = word(params, START);
        let count: Vec<u8> = (0..geom.num_agents).flat_map(|_| start.to_le_bytes()).collect();
        vec![Vec::new(), Vec::new(), count]
    }

    fn index_cell_size(&self, _params: &[ParamValue]) -> f32 {
        1.0
    }

    fn pass_params_bytes(&self, _pass: PassId, _ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
        [ELEMENTS, word(params, AMOUNT), 0, 0]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    fn run_params_bytes(
        &self,
        pass: AgentPassId,
        ctx: PassCtx<'_>,
        _run: AgentRun,
        params: &[ParamValue],
    ) -> Result<Vec<u8>, String> {
        if word(params, AMOUNT) == REFUSED_AMOUNT {
            return Err("the amount cannot be written".to_owned());
        }
        let pass = match pass {
            AgentPassId::Step(index) => PassId::Step(index),
            _ => PassId::Display,
        };
        Ok(self.pass_params_bytes(pass, ctx, params))
    }

    fn stats(&self, sums: &[f32], _counters: &[u32], _geom: &Geometry) -> Vec<StatValue> {
        vec![StatValue::Scalar(f64::from(sums[0]))]
    }
}

/// Returns the test agents model: one step that adds the amount to every agent, and a reduce that sums them.
fn agent_spec() -> Arc<GpuAgentSpec> {
    let step = ShaderSpec::new(
        AGENT_STEP,
        vec![
            storage("count_in", true),
            storage("count_out", false),
            uniform("params"),
        ],
    );
    Arc::new(count_agent_spec(
        "live_param_agents",
        "add_step",
        step,
        Box::new(AgentTestHooks),
    ))
}

/// Values of both test models' parameters: an amount of `amount` and a start of 5.
fn params(amount: u32) -> Vec<ParamValue> {
    vec![ParamValue::U32(amount), ParamValue::U32(5)]
}

/// Steps `state` 3 times at an amount of 2, sets the amount to 7 live, and steps it 4 more times, then checks that
/// the tick and the buffers stayed and the later steps added 7, with a reload-only parameter and an index past the
/// parameters rejected.
fn assert_live_edit(ctx: &GpuContext, state: &mut dyn GpuSimState, what: &str) {
    let elements = f64::from(ELEMENTS);
    steps_then_stats(ctx, state, 3);
    assert_eq!(
        scalar_stat(state, 0),
        elements * f64::from(5 + 3 * 2),
        "{what}: 3 steps at 2"
    );

    assert!(
        state.set_param(AMOUNT, &ParamValue::U32(7)),
        "{what}: the amount is live"
    );
    assert!(
        !state.set_param(START, &ParamValue::U32(50)),
        "{what}: the start is reload-only"
    );
    assert!(
        !state.set_param(2, &ParamValue::U32(1)),
        "{what}: an index past the parameters"
    );
    assert_eq!(state.tick(), 3, "{what}: a live edit keeps the tick");

    steps_then_stats(ctx, state, 4);
    assert_eq!(state.tick(), 7, "{what}: the steps go on from the tick");
    assert_eq!(
        scalar_stat(state, 0),
        elements * f64::from(5 + 3 * 2 + 4 * 7),
        "{what}: the steps after the edit add the new amount to the kept values"
    );
}

/// A live edit on either engine keeps the tick and the buffers, and every step after it reads the new value, the
/// counting step of a batch included.
#[test]
fn a_live_edit_keeps_the_run_and_later_steps_read_the_new_value() {
    let Some(ctx) = headless_context("live_param_tests") else {
        return;
    };
    let live = vec![true, false];
    let mut grid = GpuGridEngine::new(&ctx, grid_spec(), &params(2), None).with_live_params(live.clone());
    assert_live_edit(&ctx, &mut grid, "grid");
    // The fire word counts the amount once per cell in the batch's last step, through the counting block.
    assert_eq!(grid.fire_counts().counts, [7 * ELEMENTS], "grid: the counting block");

    let mut agents = GpuAgentEngine::new(&ctx, agent_spec(), &params(2), None).with_live_params(live);
    assert_live_edit(&ctx, &mut agents, "agents");
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// An engine built with no live parameters rejects every edit, as a model type's engine does.
#[test]
fn an_engine_without_live_parameters_rejects_every_edit() {
    let Some(ctx) = headless_context("live_param_tests") else {
        return;
    };
    let mut grid = GpuGridEngine::new(&ctx, grid_spec(), &params(2), None);
    assert!(!grid.set_param(AMOUNT, &ParamValue::U32(7)));
    steps_then_stats(&ctx, &mut grid, 2);
    assert_eq!(
        scalar_stat(&grid, 0),
        f64::from(ELEMENTS * (5 + 2 * 2)),
        "the run kept its amount"
    );

    let mut agents = GpuAgentEngine::new(&ctx, agent_spec(), &params(2), None);
    assert!(!agents.set_param(AMOUNT, &ParamValue::U32(7)));
}

/// A value at which the agents model's hooks cannot write a block is rejected, and the run keeps the value it had.
#[test]
fn a_value_whose_blocks_fail_is_rejected_and_the_run_keeps_its_value() {
    let Some(ctx) = headless_context("live_param_tests") else {
        return;
    };
    let mut agents = GpuAgentEngine::new(&ctx, agent_spec(), &params(2), None).with_live_params(vec![true, false]);
    assert!(!agents.set_param(AMOUNT, &ParamValue::U32(REFUSED_AMOUNT)));
    steps_then_stats(&ctx, &mut agents, 2);
    assert_eq!(
        scalar_stat(&agents, 0),
        f64::from(ELEMENTS * (5 + 2 * 2)),
        "the run kept its amount"
    );
    assert!(ctx.faults.take().is_none(), "a rejected edit reports no fault");
}
