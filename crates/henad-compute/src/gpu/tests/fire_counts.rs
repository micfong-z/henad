//! The fire-count channel of both GPU engines: the last step of each batch counts into the spec's fire range through
//! its counting block, and the counts read back with the stats, with the tick of the step they counted. A runner
//! publishes both with the tick of its snapshot.

use std::sync::Arc;
use std::time::{Duration, Instant};

use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{Geometry, PassCtx, PassId};
use henad_core::params::ParamValue;
use henad_core::view::StatValue;

use crate::cpu::sim_thread::SimCommand;
use crate::gpu::agent_engine::GpuAgentEngine;
use crate::gpu::agent_spec::{AgentHooks, AgentPassId, AgentRun, GpuAgentSpec};
use crate::gpu::grid_engine::GpuGridEngine;
use crate::gpu::grid_spec::{GpuGridSpec, GridHooks, GridPassCtx, GridPassId};
use crate::gpu::shader_spec::ShaderSpec;
use crate::gpu::sim_thread::{GpuBatchSettings, GpuSimState, GpuSimThread};
use crate::gpu::tests::support::{count_agent_spec, count_grid_spec, scalar_stat, steps_then_stats, storage, uniform};
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

/// Returns the test grid model: one step that counts the cells' values into word 1, and a reduce pass that sums them
/// into word 0.
fn grid_spec() -> Arc<GpuGridSpec> {
    count_grid_spec("fire_count_grid", "count_step", GRID_STEP, Box::new(GridTestHooks))
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
    let step = ShaderSpec::new(
        AGENT_STEP,
        vec![
            storage("count_in", true),
            storage("count_out", false),
            storage("counters", false),
            uniform("params"),
        ],
    );
    let mut spec = count_agent_spec("fire_count_agents", "count_step", step, Box::new(AgentTestHooks));
    spec.counters = 2;
    spec.fire_counters = FIRE_WORD..FIRE_WORD + 1;
    Arc::new(spec)
}

/// Returns the fire count of the step that reached `tick`: the values every element held as the step began.
fn count_of_step(tick: u64) -> u32 {
    SEED_TOTAL + (tick as u32 - 1) * ELEMENTS
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
            scalar_stat(state, 0),
            f64::from(SEED_TOTAL + tick as u32 * ELEMENTS),
            "{what}: the stats after a call of {calls} steps"
        );
    }
}

/// Each engine counts the last step of each `encode_steps` call, whatever its length, and never sums the steps of a
/// call.
#[test]
fn a_counted_step_reads_the_last_steps_count_in_calls_of_1_3_and_64() {
    let Some(ctx) = headless_context("fire_count_tests") else {
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
    let Some(ctx) = headless_context("fire_count_tests") else {
        return;
    };
    let mut grid = GpuGridEngine::new(&ctx, grid_spec(), &[], None);
    steps_then_stats(&ctx, &mut grid, 5);
    let counted = FireCounts::new(5, vec![count_of_step(5)]);
    assert_eq!(grid.fire_counts(), counted);
    assert_eq!(scalar_stat(&grid, 0), f64::from(SEED_TOTAL + 5 * ELEMENTS));
    for read in 1..=2 {
        steps_then_stats(&ctx, &mut grid, 0);
        assert_eq!(grid.fire_counts(), counted, "stats read {read} with no step");
        assert_eq!(
            scalar_stat(&grid, 0),
            f64::from(SEED_TOTAL + 5 * ELEMENTS),
            "stats read {read}: the tally is taken afresh"
        );
    }
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// A spec with no fire range counts nothing, and its counts are empty.
#[test]
fn a_spec_with_no_fire_range_has_empty_counts() {
    let Some(ctx) = headless_context("fire_count_tests") else {
        return;
    };
    let mut spec = Arc::try_unwrap(grid_spec()).unwrap_or_else(|_| unreachable!("the spec is not shared"));
    spec.fire_counters = 0..0;
    let mut grid = GpuGridEngine::new(&ctx, Arc::new(spec), &[], None);
    steps_then_stats(&ctx, &mut grid, 3);
    assert!(grid.fire_counts().is_empty());
    assert_eq!(scalar_stat(&grid, 0), f64::from(SEED_TOTAL + 3 * ELEMENTS));
}

/// Takes the snapshots `thread` publishes for `duration`, and appends each one's tick, first stat and fire counts to
/// `seen`.
fn take_snapshots_for(thread: &mut GpuSimThread, duration: Duration, seen: &mut Vec<(u64, f64, FireCounts)>) {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        if let Some(snap) = thread.take_snapshot() {
            let total = match snap.stats[0].value() {
                StatValue::Scalar(value) => *value,
                other => panic!("not a scalar: {other:?}"),
            };
            seen.push((snap.tick, total, snap.fire_counts));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A playing runner publishes each snapshot with the stats and the fire counts of its own tick, capped at the rate the
/// editor starts at and uncapped.
#[test]
fn a_playing_runner_publishes_the_stats_of_each_snapshots_own_tick() {
    let Some(ctx) = headless_context("fire_count_runner_test") else {
        return;
    };
    let grid = GpuGridEngine::new(&ctx, grid_spec(), &[], None);
    let faults = ctx.faults.clone();
    let mut thread = GpuSimThread::new(ctx, Box::new(grid), GpuBatchSettings::default(), None, faults.clone());
    let mut seen = Vec::new();
    thread.send(SimCommand::SetUncapped(false));
    thread.send(SimCommand::SetTargetTps(30.0));
    thread.play();
    take_snapshots_for(&mut thread, Duration::from_millis(500), &mut seen);
    let capped = seen.len();
    thread.send(SimCommand::SetUncapped(true));
    take_snapshots_for(&mut thread, Duration::from_millis(200), &mut seen);
    thread.pause();
    take_snapshots_for(&mut thread, Duration::from_millis(100), &mut seen);

    assert!(capped >= 5, "half a second at 30 TPS published {capped} snapshots");
    assert!(seen.len() > capped + 1, "the uncapped run published nothing");
    for (tick, total, counts) in &seen {
        let tick = *tick;
        assert_eq!(
            *total,
            f64::from(SEED_TOTAL + tick as u32 * ELEMENTS),
            "the snapshot at tick {tick} holds the stats of another tick"
        );
        let counted = if tick == 0 {
            FireCounts::new(0, vec![0])
        } else {
            FireCounts::new(tick, vec![count_of_step(tick)])
        };
        assert_eq!(
            *counts, counted,
            "the snapshot at tick {tick} holds the counts of another step"
        );
    }
    assert!(faults.take().is_none(), "no fault reached the sink");
}
