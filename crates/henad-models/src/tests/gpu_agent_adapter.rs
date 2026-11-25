//! The example GPU agent models as [`GpuAgentState`] reads them, and an entry built through the public constructors
//! from owned copies of their declarations, compared with the registered entry.

use std::any::type_name;
use std::marker::PhantomData;
use std::sync::Arc;

use henad_compute::entry::{EntryDeclarations, ModelEntry, ModelState, register_gpu_agent_model};
use henad_compute::gpu::{
    AgentActionSpec, AgentBufferSpec, AgentDisplaySpec, AgentHooks, AgentPassSpec, AgentReduceSpec, GpuAgentSpec,
    GpuAgentState, GpuContext, GpuNeeds, GpuSimState, ShaderBinding, ShaderSpec, SideTable,
};
use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{Geometry, GpuAgentModel, PassCtx, PassId};
use henad_core::explore::fingerprint::schema_hash;
use henad_core::model::SimState as _;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::provenance::ModelSource;
use henad_core::topology::TopologyHint;
use henad_core::view::{StatDescriptor, StatValue};

use super::support::baseline_device;
use crate::gpu_ants::GpuAnts;
use crate::gpu_boids::GpuBoids;

/// Population of every built case. It passes one workgroup.
const AGENTS: u32 = 2_000;
/// World side of every built case.
const WORLD: f32 = 200.0;
/// Seed of every built case.
const SEED: u64 = 7;
/// Steps a case compares its stats over.
const STEPS: usize = 8;
/// Largest relative distance between two runs of a model that does not replay exactly, over [`STEPS`].
const INEXACT_STATS_TOLERANCE: f64 = 0.02;

/// Hooks of model `M`, as a host outside henad-compute writes them.
struct OwnedHooks<M>(PhantomData<M>);

impl<M: GpuAgentModel> AgentHooks for OwnedHooks<M> {
    fn dims(&self, params: &[ParamValue]) -> (u32, Extent) {
        M::dims(params)
    }

    fn buffer_lens(&self, geom: &Geometry) -> Vec<usize> {
        M::buffer_lens(geom)
    }

    fn seed_buffers(&self, geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
        M::seed_buffers(geom, params, seed)
    }

    fn index_cell_size(&self, params: &[ParamValue]) -> f32 {
        M::index_cell_size(params)
    }

    fn pass_params_bytes(&self, pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
        M::pass_params_bytes(pass, ctx, params)
    }

    fn stats(&self, sums: &[f32], counters: &[u32], geom: &Geometry) -> Vec<StatValue> {
        M::stats(sums, counters, geom)
    }
}

/// Returns a shader that owns its source and its binding names.
fn owned_shader(source: &str, bindings: &[BindingDecl]) -> ShaderSpec {
    ShaderSpec::new(
        source.to_owned(),
        bindings
            .iter()
            .map(|decl| ShaderBinding::new(decl.name.to_owned(), decl.kind))
            .collect(),
    )
}

/// Returns the spec of `M` built through the public constructors, with every string owned.
fn owned_spec<M: GpuAgentModel>() -> GpuAgentSpec {
    let count = M::BUFFERS.len();
    let mut spec = GpuAgentSpec::new(
        M::ID.to_owned(),
        M::BUFFERS
            .iter()
            .map(|buffer| AgentBufferSpec::new(buffer.label.to_owned(), buffer.double_buffered, buffer.drawable))
            .collect(),
        M::POS_BUFFER,
        M::COLOR_BUFFER,
        AgentReduceSpec::new(
            owned_shader(M::REDUCE.shader, M::REDUCE.bindings),
            M::REDUCE.lanes,
            M::REDUCE.domain,
            SideTable::swapping(count),
        ),
        M::STATS
            .iter()
            .map(|stat| StatDescriptor::owned(stat.label().to_owned(), stat.color()))
            .collect::<Vec<_>>(),
        Box::new(OwnedHooks::<M>(PhantomData)),
    );
    spec.index = M::INDEX;
    spec.counters = M::COUNTERS;
    spec.steps = M::STEP_PASSES
        .iter()
        .map(|pass| {
            AgentPassSpec::new(
                pass.label.to_owned(),
                owned_shader(pass.shader, pass.bindings),
                pass.domain,
                SideTable::swapping(count),
            )
        })
        .collect();
    spec.display = M::DISPLAY.as_ref().map(|display| {
        AgentDisplaySpec::new(
            owned_shader(display.shader, display.bindings),
            display.workgroup,
            SideTable::swapping(count),
        )
    });
    spec.actions = M::ACTIONS
        .iter()
        .map(|action| {
            AgentActionSpec::new(
                action.desc.id().to_owned(),
                owned_shader(action.pass.shader, action.pass.bindings),
                action.pass.domain,
                SideTable::in_place(count),
            )
        })
        .collect();
    spec
}

/// Returns the entry of `M` built through the public constructors from owned declarations.
fn entry_from_values<M: GpuAgentModel>() -> ModelEntry {
    let spec = Arc::new(owned_spec::<M>());
    let mut declarations = EntryDeclarations::new(
        M::ID.to_owned(),
        M::NAME.to_owned(),
        M::DESCRIPTION.to_owned(),
        M::param_descriptors()
            .into_iter()
            .map(ParamDescriptor::on_reload)
            .collect(),
        TopologyHint {
            grid: M::DISPLAY.is_some(),
            agents: true,
            edges: false,
        },
        spec.structure(),
    );
    declarations.stats = M::STATS.to_vec();
    declarations.actions = M::ACTIONS.iter().map(|action| action.desc.clone()).collect();
    declarations.replays_exactly = M::REPLAYS_EXACTLY;
    let source = ModelSource::__from_type_path(type_name::<M>());
    ModelEntry::from_gpu_agent_spec(declarations, source, spec)
}

/// Returns the values of `M`'s parameters at the size of every built case.
fn params<M: GpuAgentModel>() -> Vec<ParamValue> {
    M::param_descriptors()
        .iter()
        .map(|descriptor| match descriptor.id() {
            "num_agents" => ParamValue::U32(AGENTS),
            "world_width" | "world_height" => ParamValue::F32(WORLD),
            _ => descriptor.kind().default_value(),
        })
        .collect()
}

/// Returns a state of `entry` on `ctx`, at the seed and size of every built case.
fn build<M: GpuAgentModel>(entry: &ModelEntry, ctx: &GpuContext) -> Box<dyn GpuSimState> {
    match entry.build(&params::<M>(), Some(SEED), Some(ctx)) {
        Ok(ModelState::Gpu(state)) => state,
        Ok(ModelState::Cpu(_)) => panic!("{}: a GPU model builds a GPU state", M::ID),
        Err(fault) => panic!("{}: the build failed: {fault}", M::ID),
    }
}

/// Returns the stat values of `state` after each step, with those of the seeded state first.
fn stepped_stats(state: &mut Box<dyn GpuSimState>, ctx: &GpuContext) -> Vec<f64> {
    let mut values = Vec::new();
    for step in 0..=STEPS {
        if step > 0 {
            state.step();
        }
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        state.encode_stats_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
        values.extend(state.stats().iter().flat_map(|stat| match stat.value() {
            StatValue::Scalar(value) => vec![*value],
            StatValue::Vector2D { x, y } => vec![*x, *y],
            StatValue::Histogram { .. } => panic!("a stock GPU agent model reports no histogram"),
        }));
    }
    values
}

/// Returns every buffer of `state`, in declaration order.
fn all_buffers<M: GpuAgentModel>(state: &dyn GpuSimState) -> Vec<Vec<u32>> {
    (0..M::BUFFERS.len())
        .map(|index| state.read_buffer(index).expect("a declared buffer reads back"))
        .collect()
}

/// Checks the entry of `M` built from owned values against the registered one, and the registered one against a state
/// the adapter builds.
fn assert_reads_like_registered<M: GpuAgentModel>(ctx: &GpuContext) {
    let from_values = entry_from_values::<M>();
    let registered = register_gpu_agent_model::<M>();

    assert_eq!(
        format!("{from_values:?}"),
        format!("{registered:?}"),
        "{}: the two entries hold the same declarations, metadata, needs and source",
        M::ID
    );
    assert_eq!(
        schema_hash(&from_values.schema()),
        schema_hash(&registered.schema()),
        "{}: schema hash",
        M::ID
    );
    assert_eq!(
        from_values.gpu_needs(),
        registered.gpu_needs(),
        "{}: device needs",
        M::ID
    );
    assert_eq!(
        Some(GpuNeeds::with_storage_buffers(
            GpuAgentState::<M>::max_storage_bindings()
        )),
        registered.gpu_needs(),
        "{}: the adapter asks for the needs of the entry",
        M::ID
    );
    let limits = ctx.device.limits();
    let values = params::<M>();
    let adapter_demand = format!("{:?}", GpuAgentState::<M>::demand(&values, &limits));
    assert_eq!(
        format!("{:?}", from_values.demand(&values, &limits)),
        format!("Some({adapter_demand})"),
        "{}: demand from values",
        M::ID
    );
    assert_eq!(
        format!("{:?}", registered.demand(&values, &limits)),
        format!("Some({adapter_demand})"),
        "{}: registered demand",
        M::ID
    );

    let mut owned = build::<M>(&from_values, ctx);
    let mut stock = build::<M>(&registered, ctx);
    let adapter = GpuAgentState::<M>::new_seeded(ctx, &values, Some(SEED));
    let seeded = all_buffers::<M>(&*stock);
    assert_eq!(all_buffers::<M>(&*owned), seeded, "{}: seeded from values", M::ID);
    assert_eq!(all_buffers::<M>(&adapter), seeded, "{}: seeded by the adapter", M::ID);
    assert_eq!(owned.heap_bytes(), stock.heap_bytes(), "{}: heap bytes", M::ID);
    assert_eq!(
        adapter.heap_bytes(),
        stock.heap_bytes(),
        "{}: adapter heap bytes",
        M::ID
    );

    let owned_values = stepped_stats(&mut owned, ctx);
    let stock_values = stepped_stats(&mut stock, ctx);
    assert_eq!(owned_values.len(), stock_values.len(), "{}: stat count", M::ID);
    if M::REPLAYS_EXACTLY {
        assert_eq!(owned_values, stock_values, "{}: stepped stats", M::ID);
        assert_eq!(
            all_buffers::<M>(&*owned),
            all_buffers::<M>(&*stock),
            "{}: buffers after the steps",
            M::ID
        );
    } else {
        // Two runs differ in the order their neighbour walks visit agents, and only by that much.
        for (position, (owned, stock)) in owned_values.iter().zip(&stock_values).enumerate() {
            let scale = owned.abs().max(stock.abs()).max(1.0);
            assert!(
                (owned - stock).abs() <= INEXACT_STATS_TOLERANCE * scale,
                "{}: stat value {position} is {owned} from values and {stock} registered",
                M::ID
            );
        }
    }
    assert!(ctx.faults.take().is_none(), "{}: no fault reached the sink", M::ID);
}

/// Each example GPU agent model built through the public constructors reads like its registered entry.
#[test]
fn an_owned_spec_entry_reads_like_the_registered_one() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    assert_reads_like_registered::<GpuAnts>(&ctx);
    assert_reads_like_registered::<GpuBoids>(&ctx);
}

/// A press through the entry of a model built from values matches a press through the registered entry.
#[test]
fn a_press_through_an_owned_spec_matches_the_registered_entry() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let mut owned = build::<GpuAnts>(&entry_from_values::<GpuAnts>(), &ctx);
    let mut stock = build::<GpuAnts>(&register_gpu_agent_model::<GpuAnts>(), &ctx);
    for state in [&mut owned, &mut stock] {
        state.step();
        assert!(state.act(0), "the model declares action 0");
        state.step();
    }
    assert_eq!(all_buffers::<GpuAnts>(&*owned), all_buffers::<GpuAnts>(&*stock));
    assert!(ctx.faults.take().is_none(), "no fault reached the sink");
}
