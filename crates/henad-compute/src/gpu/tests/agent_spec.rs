//! The GPU agent engine built from a [`GpuAgentSpec`] of a small hand-written model: the side tables, the swap at the
//! end of a step, and the calls it makes to its hooks.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use henad_core::authoring::model::binding::BindingKind;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{Domain, Geometry, PassCtx, PassId};
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::view::{StatDescriptor, StatValue};

use crate::gpu::agent_engine::GpuAgentEngine;
use crate::gpu::agent_spec::{
    AgentActionSpec, AgentBufferSpec, AgentHooks, AgentPassId, AgentPassSpec, AgentReduceSpec, AgentRun, GpuAgentSpec,
};
use crate::gpu::shader_spec::{ShaderBinding, ShaderSpec};
use crate::gpu::sides::{BufferSide, SideRow, SideTable};
use crate::gpu::sim_thread::GpuSimState as _;
use crate::gpu::{GpuContext, MAX_STEPS_PER_SUBMISSION, headless_context, stepping};

/// Number of agents of the test model. One workgroup covers them.
const AGENTS: u32 = 16;

// Buffer positions of the test model.
const POS: usize = 0;
const COLOR: usize = 1;
const COUNT: usize = 2;
const TIMES_TEN: usize = 3;

const HEADER: &str = "struct Params { n: u32, seed: u32, _pad0: u32, _pad1: u32 }\n";

/// Writes `count + 1` to the next side of `count`.
const WRITE_SHADER: &str = r"
@group(0) @binding(0) var<storage, read> count_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> count_out: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    if (lid.x < params.n) {
        count_out[lid.x] = count_in[lid.x] + 1u;
    }
}
";

/// Writes `count * 10` to the plain buffer `times_ten`, from the side the pass's table picks.
const AFTER_SHADER: &str = r"
@group(0) @binding(0) var<storage, read> count: array<u32>;
@group(0) @binding(1) var<storage, read_write> times_ten: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    if (lid.x < params.n) {
        times_ten[lid.x] = count[lid.x] * 10u;
    }
}
";

/// Adds 100 to the side of `count` the action's table picks.
const ACTION_SHADER: &str = r"
@group(0) @binding(0) var<storage, read_write> count: array<u32>;
@group(0) @binding(1) var<uniform> params: Params;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    if (lid.x < params.n) {
        count[lid.x] = count[lid.x] + 100u;
    }
}
";

/// Sums `count` into the one partial of the single workgroup.
const REDUCE_SHADER: &str = r"
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

/// Records the fault code 3 in word 0 of the counters, as a graph model's radius fold records its fault.
const FAULT_SHADER: &str = r"
@group(0) @binding(0) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(1) var<uniform> params: Params;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    if (lid.x == 0u && params.n > 0u) {
        atomicMax(&counters[0], 3u);
    }
}
";

/// Calls the engine made to each hook.
#[derive(Default)]
struct HookCalls {
    dims: AtomicUsize,
    buffer_lens: AtomicUsize,
    seed_buffers: AtomicUsize,
    index_cell_size: AtomicUsize,
    pass_params_bytes: AtomicUsize,
    stats: AtomicUsize,
}

impl HookCalls {
    /// Returns the calls of every hook, in declaration order.
    fn counts(&self) -> [usize; 6] {
        [
            &self.dims,
            &self.buffer_lens,
            &self.seed_buffers,
            &self.index_cell_size,
            &self.pass_params_bytes,
            &self.stats,
        ]
        .map(|calls| calls.load(Ordering::Relaxed))
    }
}

/// Hooks of the test model, which seed `count` with `1, 2, ...` and record each call.
struct CountingHooks {
    calls: Arc<HookCalls>,
}

impl AgentHooks for CountingHooks {
    fn dims(&self, _params: &[ParamValue]) -> (u32, Extent) {
        self.calls.dims.fetch_add(1, Ordering::Relaxed);
        (AGENTS, Extent { w: 8.0, h: 8.0 })
    }

    fn buffer_lens(&self, geom: &Geometry) -> Vec<usize> {
        self.calls.buffer_lens.fetch_add(1, Ordering::Relaxed);
        let agents = geom.num_agents as usize;
        vec![agents * 2, agents, agents, agents]
    }

    fn seed_buffers(&self, geom: &Geometry, _params: &[ParamValue], _seed: Option<u64>) -> Vec<Vec<u8>> {
        self.calls.seed_buffers.fetch_add(1, Ordering::Relaxed);
        let count: Vec<u8> = (1..=geom.num_agents).flat_map(u32::to_le_bytes).collect();
        vec![Vec::new(), Vec::new(), count, Vec::new()]
    }

    fn index_cell_size(&self, _params: &[ParamValue]) -> f32 {
        self.calls.index_cell_size.fetch_add(1, Ordering::Relaxed);
        2.0
    }

    fn pass_params_bytes(&self, _pass: PassId, ctx: PassCtx<'_>, _params: &[ParamValue]) -> Vec<u8> {
        self.calls.pass_params_bytes.fetch_add(1, Ordering::Relaxed);
        [AGENTS, ctx.seed, 0, 0]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    fn stats(&self, sums: &[f32], _counters: &[u32], _geom: &Geometry) -> Vec<StatValue> {
        self.calls.stats.fetch_add(1, Ordering::Relaxed);
        vec![StatValue::Scalar(f64::from(sums[0]))]
    }
}

fn storage(name: &'static str, read_only: bool) -> ShaderBinding {
    ShaderBinding::new(name, BindingKind::Storage { read_only })
}

/// Returns a shader of the test model, with the uniform block's struct in front of `body`.
fn shader(body: &str, bindings: Vec<ShaderBinding>) -> ShaderSpec {
    ShaderSpec::new(format!("{HEADER}{body}"), bindings)
}

/// Returns the test model, whose `after` pass reads `count` from `after_reads`.
///
/// `count` is the one double buffered buffer. A label is owned, as a model composed at run time holds it.
fn spec(after_reads: BufferSide, calls: &Arc<HookCalls>) -> Arc<GpuAgentSpec> {
    let uniform = || ShaderBinding::new("params", BindingKind::Uniform);
    let buffers = vec![
        AgentBufferSpec::new("pos".to_owned(), false, true),
        AgentBufferSpec::new("color".to_owned(), false, true),
        AgentBufferSpec::new("count".to_owned(), true, false),
        AgentBufferSpec::new("times_ten".to_owned(), false, false),
    ];
    let reduce = AgentReduceSpec::new(
        shader(
            REDUCE_SHADER,
            vec![storage("count", true), storage("partials", false), uniform()],
        ),
        1,
        Domain::Agents,
        SideTable::swapping(buffers.len()),
    );
    let mut spec = GpuAgentSpec::new(
        "side_table_probe".to_owned(),
        buffers,
        POS,
        COLOR,
        reduce,
        Cow::Owned(vec![StatDescriptor::owned("Total".to_owned(), [1, 2, 3, 255])]),
        Box::new(CountingHooks {
            calls: Arc::clone(calls),
        }),
    );
    spec.steps = vec![
        AgentPassSpec::new(
            "write".to_owned(),
            shader(
                WRITE_SHADER,
                vec![storage("count_in", true), storage("count_out", false), uniform()],
            ),
            Domain::Agents,
            SideTable::swapping(spec.buffers.len()),
        ),
        AgentPassSpec::new(
            "after".to_owned(),
            shader(
                AFTER_SHADER,
                vec![storage("count", true), storage("times_ten", false), uniform()],
            ),
            Domain::Agents,
            SideTable::new(vec![SideRow::new(after_reads, BufferSide::Next); spec.buffers.len()]),
        ),
    ];
    spec.actions = vec![AgentActionSpec::new(
        "bump".to_owned(),
        shader(ACTION_SHADER, vec![storage("count", false), uniform()]),
        Domain::Agents,
        SideTable::in_place(spec.buffers.len()),
    )];
    Arc::new(spec)
}

fn context() -> Option<GpuContext> {
    headless_context("agent_spec_tests", wgpu::Features::empty())
}

/// Returns the words of buffer `index` of `engine`.
fn words(engine: &GpuAgentEngine, index: usize) -> Vec<u32> {
    GpuAgentEngine::read_buffer(engine, index)
}

/// Returns `1 + offset, 2 + offset, ...` for every agent, each times `factor`.
fn expected(offset: u32, factor: u32) -> Vec<u32> {
    (1..=AGENTS).map(|seed| (seed + offset) * factor).collect()
}

/// A pass after the writer reads the start of the step under one orientation and the writer's output under the table
/// that points it at the next side. The sides swap once per step either way.
#[test]
fn a_side_table_picks_the_side_a_later_pass_reads() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_side_table_picks_the_side_a_later_pass_reads: no adapter");
        return;
    };
    let calls = Arc::new(HookCalls::default());

    let mut single = GpuAgentEngine::new(&ctx, spec(BufferSide::Current, &calls), &[], None);
    let mut next = GpuAgentEngine::new(&ctx, spec(BufferSide::Next, &calls), &[], None);
    assert_eq!(words(&single, COUNT), expected(0, 1), "seeded on the current side");

    for tick in 1..=5 {
        single.step();
        next.step();
        assert_eq!(single.tick(), u64::from(tick));
        assert_eq!(
            words(&single, COUNT),
            expected(tick, 1),
            "single orientation, tick {tick}"
        );
        assert_eq!(words(&next, COUNT), expected(tick, 1), "next side table, tick {tick}");
        assert_eq!(
            words(&single, TIMES_TEN),
            expected(tick - 1, 10),
            "a read of the current side sees the start of the step"
        );
        assert_eq!(
            words(&next, TIMES_TEN),
            expected(tick, 10),
            "a read of the next side sees the writer's output"
        );
    }
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// An action writes the side that holds the state on either orientation of the swap, and a press survives the next
/// step. The stats read that side too.
#[test]
fn an_in_place_action_writes_the_current_side() {
    let Some(ctx) = context() else {
        log::warn!("skipping an_in_place_action_writes_the_current_side: no adapter");
        return;
    };
    let calls = Arc::new(HookCalls::default());
    let mut engine = GpuAgentEngine::new(&ctx, spec(BufferSide::Next, &calls), &[], None);

    for tick in 0..2 {
        let before = words(&engine, COUNT);
        assert!(engine.act(0), "action 0 is declared");
        let after = words(&engine, COUNT);
        let raised: Vec<u32> = before.iter().map(|word| word + 100).collect();
        assert_eq!(after, raised, "a press at tick {tick}");
        engine.step();
        let stepped: Vec<u32> = after.iter().map(|word| word + 1).collect();
        assert_eq!(words(&engine, COUNT), stepped, "the step after a press at tick {tick}");

        engine.refresh_stats();
        let total: u32 = stepped.iter().sum();
        let entries = engine.stats();
        assert_eq!(entries[0].label(), "Total");
        assert!(
            (entries[0].value().scalar() - f64::from(total)).abs() < 1e-3,
            "{:?} against {total}",
            entries[0].value()
        );
    }
    assert!(!engine.act(1), "action 1 is not declared");
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// A build calls the size and seed hooks, and from then on only a press and a stats read call a hook.
#[test]
fn a_step_calls_no_hook() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_step_calls_no_hook: no adapter");
        return;
    };
    let calls = Arc::new(HookCalls::default());
    let mut engine = GpuAgentEngine::new(&ctx, spec(BufferSide::Next, &calls), &[], None);
    assert_eq!(calls.seed_buffers.load(Ordering::Relaxed), 1, "one seed per build");
    assert_eq!(
        calls.index_cell_size.load(Ordering::Relaxed),
        0,
        "the model declares no index"
    );

    let built = calls.counts();
    engine.step();
    engine.run_batched(MAX_STEPS_PER_SUBMISSION * 2 + 1);
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    engine.encode_steps(&mut encoder, 3, None);
    engine.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    drop(engine.view());
    assert_eq!(calls.counts(), built, "stepping and encoding a snapshot call no hook");

    let pressed = calls.pass_params_bytes.load(Ordering::Relaxed);
    assert!(engine.act(0));
    assert_eq!(
        calls.pass_params_bytes.load(Ordering::Relaxed),
        pressed + 1,
        "a press writes one uniform block"
    );
    let read = calls.stats.load(Ordering::Relaxed);
    drop(engine.stats());
    assert_eq!(
        calls.stats.load(Ordering::Relaxed),
        read + 1,
        "a stats read calls one hook"
    );
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// The runner interface reads a declared buffer, and returns nothing past the last one.
#[test]
fn the_runner_interface_reads_a_buffer() {
    let Some(ctx) = context() else {
        log::warn!("skipping the_runner_interface_reads_a_buffer: no adapter");
        return;
    };
    let calls = Arc::new(HookCalls::default());
    let engine = GpuAgentEngine::new(&ctx, spec(BufferSide::Next, &calls), &[], None);
    assert_eq!(
        crate::gpu::GpuSimState::read_buffer(&engine, COUNT),
        Some(expected(0, 1))
    );
    assert_eq!(crate::gpu::GpuSimState::read_buffer(&engine, TIMES_TEN + 1), None);
    assert_eq!(engine.population(), u64::from(AGENTS));
}

/// Where the hooks of [`faulting_spec`] cannot compute a uniform block.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockFault {
    /// Every block has a value.
    None,
    /// The step's block, at the build.
    Step,
    /// The action's block, from the first press on.
    Pressed,
}

/// Hooks of a model whose shaders record faults in the counters, whose blocks fail as `block_fault` says.
struct FaultHooks {
    block_fault: BlockFault,
}

impl AgentHooks for FaultHooks {
    fn dims(&self, _params: &[ParamValue]) -> (u32, Extent) {
        (AGENTS, Extent { w: 8.0, h: 8.0 })
    }

    fn buffer_lens(&self, geom: &Geometry) -> Vec<usize> {
        let agents = geom.num_agents as usize;
        vec![agents * 2, agents, agents, agents]
    }

    fn seed_buffers(&self, _geom: &Geometry, _params: &[ParamValue], _seed: Option<u64>) -> Vec<Vec<u8>> {
        vec![Vec::new(); 4]
    }

    fn index_cell_size(&self, _params: &[ParamValue]) -> f32 {
        1.0
    }

    fn pass_params_bytes(&self, _pass: PassId, _ctx: PassCtx<'_>, _params: &[ParamValue]) -> Vec<u8> {
        [AGENTS, 0, 0, 0].iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn stats(&self, sums: &[f32], _counters: &[u32], _geom: &Geometry) -> Vec<StatValue> {
        vec![StatValue::Scalar(f64::from(sums[0]))]
    }

    fn run_params_bytes(
        &self,
        pass: AgentPassId,
        ctx: PassCtx<'_>,
        run: AgentRun,
        params: &[ParamValue],
    ) -> Result<Vec<u8>, String> {
        let fails = match (pass, self.block_fault) {
            (AgentPassId::Step(_), BlockFault::Step) => true,
            (AgentPassId::Action(_), BlockFault::Pressed) => run.presses > 0,
            _ => false,
        };
        if fails {
            return Err("node n9 in pass 'probe' faulted: the block has no value".to_owned());
        }
        Ok(self.pass_params_bytes(PassId::Display, ctx, params))
    }

    fn fault(&self, counters: &[u32]) -> Option<String> {
        let code = *counters.first()?;
        (code != 0).then(|| format!("the pass 'probe' recorded the fault {code}"))
    }

    fn reports_faults(&self) -> bool {
        true
    }
}

/// Returns a model whose one step records a fault in the counters, with the buffers and action of [`spec`], `count`
/// on one side, and the hooks [`FaultHooks`].
fn faulting_spec(block_fault: BlockFault) -> Arc<GpuAgentSpec> {
    let uniform = || ShaderBinding::new("params", BindingKind::Uniform);
    let buffers = vec![
        AgentBufferSpec::new("pos".to_owned(), false, true),
        AgentBufferSpec::new("color".to_owned(), false, true),
        AgentBufferSpec::new("count".to_owned(), false, false),
        AgentBufferSpec::new("times_ten".to_owned(), false, false),
    ];
    let reduce = AgentReduceSpec::new(
        shader(
            REDUCE_SHADER,
            vec![storage("count", true), storage("partials", false), uniform()],
        ),
        1,
        Domain::Agents,
        SideTable::swapping(buffers.len()),
    );
    let mut spec = GpuAgentSpec::new(
        "fault_probe".to_owned(),
        buffers,
        POS,
        COLOR,
        reduce,
        Cow::Owned(vec![StatDescriptor::owned("Total".to_owned(), [1, 2, 3, 255])]),
        Box::new(FaultHooks { block_fault }),
    );
    spec.counters = 1;
    spec.steps = vec![AgentPassSpec::new(
        "probe".to_owned(),
        shader(FAULT_SHADER, vec![storage("counters", false), uniform()]),
        Domain::Agents,
        SideTable::swapping(spec.buffers.len()),
    )];
    spec.actions = vec![AgentActionSpec::new(
        "bump".to_owned(),
        shader(ACTION_SHADER, vec![storage("count", false), uniform()]),
        Domain::Agents,
        SideTable::in_place(spec.buffers.len()),
    )];
    Arc::new(spec)
}

/// A fault a shader records ends a run that steps and reads no stats, as a benchmark runs, with the hooks' message.
#[test]
fn a_recorded_fault_ends_a_run_that_reads_no_stats() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_recorded_fault_ends_a_run_that_reads_no_stats: no adapter");
        return;
    };
    let mut engine = GpuAgentEngine::new(&ctx, faulting_spec(BlockFault::None), &[], None);
    let fault = stepping::run_steps(&mut engine, &ctx, 3).expect_err("the recorded fault ends the run");
    let message = fault.to_string();
    assert!(
        message.contains("fault_probe: the pass 'probe' recorded the fault 3"),
        "{message}"
    );
}

/// A step's uniform block the hooks cannot compute fails the build with the hooks' reason.
#[test]
#[should_panic(expected = "fault_probe: node n9 in pass 'probe' faulted: the block has no value")]
fn a_step_block_the_hooks_cannot_compute_fails_the_build() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_step_block_the_hooks_cannot_compute_fails_the_build: no adapter");
        panic!("fault_probe: node n9 in pass 'probe' faulted: the block has no value (no adapter)");
    };
    drop(GpuAgentEngine::new(&ctx, faulting_spec(BlockFault::Step), &[], None));
}

/// An action's uniform block the hooks cannot compute refuses the press with the hooks' reason, and the action does
/// not run.
#[test]
fn an_action_block_the_hooks_cannot_compute_refuses_the_press() {
    let Some(ctx) = context() else {
        log::warn!("skipping an_action_block_the_hooks_cannot_compute_refuses_the_press: no adapter");
        return;
    };
    let mut engine = GpuAgentEngine::new(&ctx, faulting_spec(BlockFault::Pressed), &[], None);
    let before = words(&engine, COUNT);
    assert!(engine.act(0), "action 0 is declared");
    let fault = ctx.faults.take().expect("the press is refused");
    assert!(
        fault
            .to_string()
            .contains("fault_probe: node n9 in pass 'probe' faulted"),
        "{fault}"
    );
    assert_eq!(words(&engine, COUNT), before, "the action did not run");
}
