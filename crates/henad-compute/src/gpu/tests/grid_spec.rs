//! The GPU grid engine built from a [`GpuGridSpec`] of a small hand-written model: several step passes with their side
//! tables, a plain buffer, an action that reruns on the host before its passes, and a rerun that faults.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use henad_core::authoring::model::binding::BindingKind;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::view::{StatDescriptor, StatValue};

use crate::gpu::grid_engine::GpuGridEngine;
use crate::gpu::grid_spec::{
    GpuGridSpec, GridActionSpec, GridBufferSpec, GridHooks, GridPassCtx, GridPassId, GridPassSpec, RerunUploads,
};
use crate::gpu::shader_spec::{ShaderBinding, ShaderSpec};
use crate::gpu::sides::{BufferSide, SideRow, SideTable};
use crate::gpu::sim_thread::GpuSimState as _;
use crate::gpu::{GpuContext, headless_context};

// Grid of the test model. One workgroup of 8 by 8 covers it.
const WIDTH: u32 = 8;
const HEIGHT: u32 = 4;
const CELLS: usize = (WIDTH * HEIGHT) as usize;

// Buffer positions of the test model.
const COUNT: usize = 0;
const TIMES_TEN: usize = 1;
const KEEP: usize = 2;
const BUFFERS: usize = 3;

/// Value every cell of the plain buffer `keep` is seeded with.
const KEPT: u32 = 7;
/// Value a rerun of `bump` writes to every cell of `count`, before the press count is added.
const RERUN_BASE: u32 = 1000;

const HEADER: &str = "struct Params { width: u32, height: u32, seed: u32, count: u32 }\n\
                      struct Dims { grid: vec2<u32>, tex: vec2<u32> }\n";

/// Writes `count + 1` to the next side of `count`.
const WRITE_SHADER: &str = r"
@group(0) @binding(0) var<storage, read> count_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> count_out: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < params.width && id.y < params.height) {
        let i = id.y * params.width + id.x;
        count_out[i] = count_in[i] + 1u;
    }
}
";

/// Writes `count * 10` to the plain buffer `times_ten`, from the side of `count` the pass's table picks.
const AFTER_SHADER: &str = r"
@group(0) @binding(0) var<storage, read> count: array<u32>;
@group(0) @binding(1) var<storage, read_write> times_ten: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < params.width && id.y < params.height) {
        let i = id.y * params.width + id.x;
        times_ten[i] = count[i] * 10u;
    }
}
";

/// Adds 100 to `count` in place.
const ADD_SHADER: &str = r"
@group(0) @binding(0) var<storage, read_write> count: array<u32>;
@group(0) @binding(1) var<uniform> params: Params;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < params.width && id.y < params.height) {
        let i = id.y * params.width + id.x;
        count[i] = count[i] + 100u;
    }
}
";

/// Copies `count` to `times_ten`, and writes the press seed and count to the first two cells of `keep`.
const MARK_SHADER: &str = r"
@group(0) @binding(0) var<storage, read> count: array<u32>;
@group(0) @binding(1) var<storage, read_write> times_ten: array<u32>;
@group(0) @binding(2) var<storage, read_write> keep: array<u32>;
@group(0) @binding(3) var<uniform> params: Params;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < params.width && id.y < params.height) {
        let i = id.y * params.width + id.x;
        times_ten[i] = count[i];
        if (i == 0u) {
            keep[0] = params.seed;
            keep[1] = params.count;
        }
    }
}
";

/// Writes a grey texel per cell.
const DISPLAY_SHADER: &str = r"
@group(0) @binding(0) var<storage, read> count: array<u32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> dims: Dims;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < dims.tex.x && id.y < dims.tex.y) {
        let level = f32(count[id.y * dims.grid.x + id.x] % 2u);
        textureStore(output, vec2<i32>(id.xy), vec4<f32>(level, level, level, 1.0));
    }
}
";

/// Sums `count` into the one counter.
const REDUCE_SHADER: &str = r"
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

/// Press seed and count of every rerun, in order, shared by the test model's hooks and the test.
type PressLog = Arc<Mutex<Vec<(u32, u32)>>>;

/// Hooks of the test model, which seed `count` with `1, 2, ...` and `keep` with [`KEPT`], and record each press.
struct TestHooks {
    presses: PressLog,
}

impl GridHooks for TestHooks {
    fn dims(&self, _params: &[ParamValue]) -> (u32, u32) {
        (WIDTH, HEIGHT)
    }

    fn buffer_lens(&self, width: u32, height: u32) -> Vec<usize> {
        vec![(width * height) as usize; BUFFERS]
    }

    fn step_dims(&self, width: u32, height: u32) -> (u32, u32) {
        (width, height)
    }

    fn seed_buffers(&self, _width: u32, _height: u32, _params: &[ParamValue], _seed: Option<u64>) -> Vec<Vec<u32>> {
        vec![(1..=CELLS as u32).collect(), vec![0; CELLS], vec![KEPT; CELLS]]
    }

    fn pass_params_bytes(&self, _pass: GridPassId, ctx: GridPassCtx, _params: &[ParamValue]) -> Vec<u8> {
        [ctx.width, ctx.height, ctx.press.seed, ctx.press.count]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    /// Writes [`RERUN_BASE`] plus the press count to `count` for `bump`, and faults for `fail`.
    fn rerun(
        &self,
        action: usize,
        ctx: GridPassCtx,
        _params: &[ParamValue],
        stream: &mut u64,
    ) -> Result<RerunUploads, String> {
        *stream = stream.wrapping_add(1);
        self.presses
            .lock()
            .expect("the press log")
            .push((ctx.press.seed, ctx.press.count));
        match action {
            0 => Ok(vec![(COUNT, vec![RERUN_BASE + ctx.press.count; CELLS])]),
            _ => Err("the rerun faulted".to_owned()),
        }
    }

    fn stats(&self, counts: &[u32], _ctx: GridPassCtx, _params: &[ParamValue]) -> Vec<StatValue> {
        counts
            .iter()
            .map(|&count| StatValue::Scalar(f64::from(count)))
            .collect()
    }
}

fn storage(name: &'static str, read_only: bool) -> ShaderBinding {
    ShaderBinding::new(name, BindingKind::Storage { read_only })
}

fn uniform(name: &'static str) -> ShaderBinding {
    ShaderBinding::new(name, BindingKind::Uniform)
}

/// Returns a shader of the test model, with the uniform blocks' structs in front of `body`.
fn shader(body: &str, bindings: Vec<ShaderBinding>) -> ShaderSpec {
    ShaderSpec::new(format!("{HEADER}{body}"), bindings)
}

/// Returns the table of a pass that reaches `count` with `count` and every plain buffer in place.
fn table(count: SideRow) -> SideTable {
    let in_place = SideRow::new(BufferSide::Current, BufferSide::Current);
    SideTable::new(vec![count, in_place, in_place])
}

/// Returns the test model, whose `after` pass reads `count` from `after_reads`, with the press log of its hooks.
///
/// `count` is the one double buffered buffer, and `times_ten` and `keep` are plain. Every label is owned, as a model
/// composed at run time holds it.
fn spec(after_reads: BufferSide) -> (Arc<GpuGridSpec>, PressLog) {
    let swapping = table(SideRow::new(BufferSide::Current, BufferSide::Next));
    let in_place = table(SideRow::new(BufferSide::Current, BufferSide::Current));
    let presses = Arc::new(Mutex::new(Vec::new()));
    let write = GridPassSpec::new(
        "write".to_owned(),
        shader(
            WRITE_SHADER,
            vec![
                storage("count_in", true),
                storage("count_out", false),
                uniform("params"),
            ],
        ),
        swapping.clone(),
    );
    let after = GridPassSpec::new(
        "after".to_owned(),
        shader(
            AFTER_SHADER,
            vec![storage("count", true), storage("times_ten", false), uniform("params")],
        ),
        table(SideRow::new(after_reads, BufferSide::Next)),
    );
    let display = GridPassSpec::new(
        "display".to_owned(),
        shader(
            DISPLAY_SHADER,
            vec![
                storage("count", true),
                ShaderBinding::new("output", BindingKind::StorageTexture),
                uniform("dims"),
            ],
        ),
        swapping.clone(),
    );
    let reduce = GridPassSpec::new(
        "reduce".to_owned(),
        shader(
            REDUCE_SHADER,
            vec![storage("count", true), storage("counters", false), uniform("dims")],
        ),
        swapping,
    );
    let mut spec = GpuGridSpec::new(
        "grid_spec_probe".to_owned(),
        8,
        vec![
            GridBufferSpec::new("count".to_owned(), true),
            GridBufferSpec::new("times_ten".to_owned(), false),
            GridBufferSpec::new("keep".to_owned(), false),
        ],
        vec![write, after],
        display,
        reduce,
        Cow::Owned(vec![StatDescriptor::owned("Total".to_owned(), [1, 2, 3, 255])]),
        Box::new(TestHooks {
            presses: Arc::clone(&presses),
        }),
    );
    let add = GridPassSpec::new(
        "bump_add".to_owned(),
        shader(ADD_SHADER, vec![storage("count", false), uniform("params")]),
        in_place.clone(),
    );
    let mark = GridPassSpec::new(
        "bump_mark".to_owned(),
        shader(
            MARK_SHADER,
            vec![
                storage("count", true),
                storage("times_ten", false),
                storage("keep", false),
                uniform("params"),
            ],
        ),
        in_place.clone(),
    );
    let never = GridPassSpec::new(
        "fail_add".to_owned(),
        shader(ADD_SHADER, vec![storage("count", false), uniform("params")]),
        in_place,
    );
    spec.actions = vec![
        GridActionSpec::with_passes("bump".to_owned(), vec![add, mark]),
        GridActionSpec::with_passes("fail".to_owned(), vec![never]),
    ];
    (Arc::new(spec), presses)
}

/// Returns a device for these tests, `None` without an adapter.
fn context() -> Option<GpuContext> {
    headless_context("grid_spec_tests", wgpu::Features::empty())
}

/// Returns buffer `index` of `engine`, read back from its current side.
fn read(engine: &GpuGridEngine, index: usize) -> Vec<u32> {
    engine.read_buffer(index).expect("the engine holds the buffer")
}

/// Steps `engine` once in its own submission.
fn step(engine: &mut GpuGridEngine) {
    engine.step();
}

/// A pass after the writer of a double buffered buffer sees the start of the step under [`BufferSide::Current`] and
/// the writer's output under [`BufferSide::Next`]. The plain buffer `keep` keeps its seed, since nothing swaps it.
#[test]
fn a_later_step_pass_reads_the_side_its_table_picks() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_later_step_pass_reads_the_side_its_table_picks: no adapter");
        return;
    };
    for (after_reads, lag) in [(BufferSide::Current, 1), (BufferSide::Next, 0)] {
        let (spec, _) = spec(after_reads);
        let mut engine = GpuGridEngine::new(&ctx, spec, &[], None);
        for tick in 1..=5u32 {
            step(&mut engine);
            let count = read(&engine, COUNT);
            let expected: Vec<u32> = (1..=CELLS as u32).map(|start| start + tick).collect();
            assert_eq!(count, expected, "{after_reads:?}: count after tick {tick}");
            let times_ten: Vec<u32> = expected.iter().map(|value| (value - lag) * 10).collect();
            assert_eq!(read(&engine, TIMES_TEN), times_ten, "{after_reads:?}: tick {tick}");
            assert_eq!(read(&engine, KEEP), vec![KEPT; CELLS], "{after_reads:?}: tick {tick}");
        }
        assert_eq!(engine.tick(), 5);
    }
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}

/// The buffers of the dual set take two sides and the plain ones one.
#[test]
fn a_plain_buffer_takes_one_side() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_plain_buffer_takes_one_side: no adapter");
        return;
    };
    let (spec, _) = spec(BufferSide::Next);
    let demand = spec.demand(&[], &ctx.device.limits());
    let labels: Vec<&str> = demand.buffers.iter().map(|alloc| alloc.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "grid_spec_probe_buffer0_a",
            "grid_spec_probe_buffer0_b",
            "grid_spec_probe_buffer1_a",
            "grid_spec_probe_buffer2_a",
        ]
    );
    let engine = GpuGridEngine::new(&ctx, spec, &[], None);
    let texture = CELLS * 4;
    assert_eq!(engine.heap_bytes(), CELLS * 4 * 4 + texture);
}

/// Checks a press of `bump` on `engine`: the rerun's words land on the current side, both passes run on it in order,
/// and the next step starts from them.
fn assert_bump(ctx: &GpuContext, engine: &mut GpuGridEngine, press: u32) {
    assert!(engine.act(0), "bump is declared");
    let pressed = RERUN_BASE + press + 100;
    assert_eq!(
        read(engine, COUNT),
        vec![pressed; CELLS],
        "press {press}: the rerun, then add"
    );
    assert_eq!(
        read(engine, TIMES_TEN),
        vec![pressed; CELLS],
        "press {press}: mark runs after add"
    );
    step(engine);
    assert_eq!(
        read(engine, COUNT),
        vec![pressed + 1; CELLS],
        "press {press}: the step reads the press"
    );
    assert!(ctx.faults.take().is_none(), "press {press}: no fault reached the sink");
}

/// An action reruns on the host and uploads to the current side, then runs its passes in place, on either orientation
/// of the swap. Each press advances the seed and the count it hands the hooks and the uniform blocks.
#[test]
fn an_action_reruns_on_the_host_then_runs_its_passes_in_place() {
    let Some(ctx) = context() else {
        log::warn!("skipping an_action_reruns_on_the_host_then_runs_its_passes_in_place: no adapter");
        return;
    };
    let (spec, presses) = spec(BufferSide::Next);
    let mut engine = GpuGridEngine::new(&ctx, spec, &[], Some(3));
    step(&mut engine);
    assert_bump(&ctx, &mut engine, 1);
    assert_bump(&ctx, &mut engine, 2);

    let seen = presses.lock().expect("the press log").clone();
    let first = henad_core::action::action_seed(Some(3)) as u32;
    let advance = |seed: u32| seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    assert_eq!(seen, [(advance(first), 1), (advance(advance(first)), 2)]);
    let keep = read(&engine, KEEP);
    assert_eq!(
        keep[..2],
        [advance(advance(first)), 2],
        "the action uniform holds the press"
    );
    assert!(keep[2..].iter().all(|&value| value == KEPT));
}

/// A rerun that faults reports a refused fault, runs none of its action's passes, and leaves the state as it was.
#[test]
fn a_rerun_that_faults_reaches_the_sink_and_runs_no_pass() {
    let Some(ctx) = context() else {
        log::warn!("skipping a_rerun_that_faults_reaches_the_sink_and_runs_no_pass: no adapter");
        return;
    };
    let (spec, _) = spec(BufferSide::Next);
    let mut engine = GpuGridEngine::new(&ctx, spec, &[], None);
    step(&mut engine);
    let before = read(&engine, COUNT);
    assert!(engine.act(1), "fail is declared");
    let fault = ctx.faults.take().expect("the fault reached the sink");
    assert!(fault.to_string().contains("the rerun faulted"), "{fault}");
    assert_eq!(read(&engine, COUNT), before, "no pass of the action ran");
    assert!(!engine.act(2), "no third action is declared");
}

/// The reduce pass reads the current side, after the swap.
#[test]
fn the_stats_read_the_current_side() {
    let Some(ctx) = context() else {
        log::warn!("skipping the_stats_read_the_current_side: no adapter");
        return;
    };
    let (spec, _) = spec(BufferSide::Next);
    let mut engine = GpuGridEngine::new(&ctx, spec, &[], None);
    step(&mut engine);
    step(&mut engine);
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    engine.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    engine.begin_stats_readback();
    engine.poll_stats_readback(&ctx.device, true);
    let total: u32 = (1..=CELLS as u32).map(|start| start + 2).sum();
    let stats = engine.stats();
    assert!(
        matches!(stats[0].value(), StatValue::Scalar(value) if *value == f64::from(total)),
        "{stats:?}"
    );
}
