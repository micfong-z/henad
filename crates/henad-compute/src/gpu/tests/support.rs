//! Headless device for this crate's GPU tests, and the helpers and small models that several of them share.

use std::borrow::Cow;
use std::sync::Arc;

use henad_core::authoring::model::binding::BindingKind;
use henad_core::authoring::model::gpu_agent_model::Domain;
use henad_core::view::{StatDescriptor, StatValue};

use crate::fault::FaultSink;
use crate::gpu::GpuContext;
use crate::gpu::agent_spec::{AgentBufferSpec, AgentHooks, AgentPassSpec, AgentReduceSpec, GpuAgentSpec};
use crate::gpu::grid_spec::{GpuGridSpec, GridBufferSpec, GridHooks, GridPassSpec};
use crate::gpu::shader_spec::{ShaderBinding, ShaderSpec};
use crate::gpu::sides::SideTable;
use crate::gpu::sim_thread::GpuSimState;

/// Set in CI to turn "no GPU here" from a skip into a failure.
const REQUIRE_GPU: &str = "HENAD_REQUIRE_GPU";

/// Returns whether `HENAD_REQUIRE_GPU` is set. Empty counts as unset, so a workflow matrix can blank it out on
/// runners without a GPU.
pub fn gpu_required() -> bool {
    std::env::var_os(REQUIRE_GPU).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Returns the default adapter, or `None` when this machine has none, which skips the calling test.
///
/// # Panics
///
/// Panics if [`REQUIRE_GPU`] is set and no adapter is available.
pub fn headless_adapter() -> Option<wgpu::Adapter> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
    if let Err(err) = &adapter {
        assert!(
            !gpu_required(),
            "{REQUIRE_GPU} is set but no wgpu adapter is available: {err}"
        );
        log::warn!("skipping a GPU test: no adapter");
    }
    adapter.ok()
}

/// Returns a headless device, or `None` when this machine cannot provide one.
///
/// # Panics
///
/// Panics if [`REQUIRE_GPU`] is set and no adapter or device could be acquired. Missing
/// `required_features` still returns `None`, since a software rasteriser owes us nothing
/// optional.
pub fn headless_context(label: &str, required_features: wgpu::Features) -> Option<GpuContext> {
    let adapter = headless_adapter()?;

    if !adapter.features().contains(required_features) {
        log::warn!("skipping {label}: the adapter lacks {required_features:?}");
        return None;
    }

    let device = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some(label),
        required_features,
        // No binding count. henad-compute cannot see the models, and its own tests bind few.
        required_limits: crate::gpu::limits::raise(&adapter, &wgpu::Limits::default(), crate::gpu::GpuNeeds::default()),
        ..Default::default()
    }));
    if let Err(err) = &device {
        assert!(
            !gpu_required(),
            "{REQUIRE_GPU} is set but adapter '{}' gave no device: {err}",
            adapter.get_info().name
        );
    }
    let (device, queue) = device.ok()?;

    Some(GpuContext::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        FaultSink::new(),
    ))
}

/// Returns the declaration of a storage buffer binding `name`, read-only or read-write.
pub fn storage(name: &'static str, read_only: bool) -> ShaderBinding {
    ShaderBinding::new(name, BindingKind::Storage { read_only })
}

/// Returns the declaration of a uniform binding `name`.
pub fn uniform(name: &'static str) -> ShaderBinding {
    ShaderBinding::new(name, BindingKind::Uniform)
}

/// Records `steps` steps and then the stats passes into one submission, as the runner does for a snapshot, and waits
/// for the readback.
pub fn steps_then_stats(ctx: &GpuContext, state: &mut dyn GpuSimState, steps: u32) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_steps(&mut encoder, steps, None);
    state.encode_stats_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    state.poll_stats_readback(&ctx.device, true);
}

/// Returns the scalar of stat `index`.
///
/// # Panics
///
/// Panics if the stat is not a scalar.
pub fn scalar_stat(state: &dyn GpuSimState, index: usize) -> f64 {
    match state.stats()[index].value() {
        StatValue::Scalar(value) => *value,
        other => panic!("not a scalar: {other:?}"),
    }
}

/// Writes nothing to its texture, which no test reads.
const COUNT_GRID_DISPLAY: &str = r"
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

/// Sums `count` into word 0.
const COUNT_GRID_REDUCE: &str = r"
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

/// Sums `count` into the one partial of the single workgroup.
const COUNT_AGENT_REDUCE: &str = r"
struct Params { n: u32, _pad0: u32, _pad1: u32, _pad2: u32 }

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

/// Returns a grid model over one double buffered buffer, `count`, with a step pass `step_label` running `step_source`,
/// a reduce pass that sums `count` into word 0 of two counters, a fire range over word 1, and one stat.
///
/// The step binds `count_in`, `count_out`, `counters` and `params`, in that order.
pub fn count_grid_spec(
    id: &'static str,
    step_label: &'static str,
    step_source: &'static str,
    hooks: Box<dyn GridHooks>,
) -> Arc<GpuGridSpec> {
    let swapping = || SideTable::swapping(1);
    let step = GridPassSpec::new(
        step_label,
        ShaderSpec::new(
            step_source,
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
            COUNT_GRID_DISPLAY,
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
            COUNT_GRID_REDUCE,
            vec![storage("count", true), storage("counters", false), uniform("dims")],
        ),
        swapping(),
    );
    let mut spec = GpuGridSpec::new(
        id,
        8,
        vec![GridBufferSpec::new("count", true)],
        vec![step],
        display,
        reduce,
        Cow::Owned(vec![StatDescriptor::owned("Total".to_owned(), [1, 2, 3, 255])]),
        hooks,
    );
    spec.counters = 2;
    spec.fire_counters = 1..2;
    Arc::new(spec)
}

/// Returns an agents model over the drawable `pos` and `color` and the double buffered `count`, with the step pass
/// `step_label` running `step`, a reduce that sums `count` into its one lane, and one stat.
pub fn count_agent_spec(
    id: &'static str,
    step_label: &'static str,
    step: ShaderSpec,
    hooks: Box<dyn AgentHooks>,
) -> GpuAgentSpec {
    let buffers = vec![
        AgentBufferSpec::new("pos", false, true),
        AgentBufferSpec::new("color", false, true),
        AgentBufferSpec::new("count", true, false),
    ];
    let reduce = AgentReduceSpec::new(
        ShaderSpec::new(
            COUNT_AGENT_REDUCE,
            vec![storage("count", true), storage("partials", false), uniform("params")],
        ),
        1,
        Domain::Agents,
        SideTable::swapping(buffers.len()),
    );
    let mut spec = GpuAgentSpec::new(
        id,
        buffers,
        0,
        1,
        reduce,
        Cow::Owned(vec![StatDescriptor::owned("Total".to_owned(), [1, 2, 3, 255])]),
        hooks,
    );
    spec.steps = vec![AgentPassSpec::new(
        step_label,
        step,
        Domain::Agents,
        SideTable::swapping(spec.buffers.len()),
    )];
    spec
}
