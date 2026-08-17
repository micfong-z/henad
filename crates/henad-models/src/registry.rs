use henad_compute::cpu::agent_engine::{AgentModelState, agent_model_param_descriptors};
use henad_compute::cpu::grid_engine::{GridModelState, grid_model_param_descriptors};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::agent_engine::{GpuAgentModelDescriptor, GpuAgentState};
use henad_compute::gpu::capacity::Demand;
use henad_compute::gpu::grid_engine::{GpuGridModelDescriptor, GpuGridState};
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_core::authoring::model::agent_model::AgentModel;
use henad_core::authoring::model::gpu_agent_model::GpuAgentModel;
use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::model::{Model as _, SimState};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;
use henad_core::view::StatDescriptor;

/// A freshly created simulation state, tagged with which runner can drive it.
///
/// The two arms are not interchangeable: a CPU state is stepped one tick per call by
/// `henad_compute::cpu::sim_thread::SimThread`, while a GPU state has many steps *encoded into one
/// submission* by `henad_compute::gpu::GpuSimThread`. The factory returns this enum (rather than
/// a bare `Box<dyn SimState>`) so the caller can pick the right runner without downcasting, and
/// so it is impossible to hand a GPU state to the CPU thread by mistake.
pub enum ModelState {
    Cpu(Box<dyn SimState>),
    Gpu(Box<dyn GpuSimState>),
}

/// A type-erased model factory.
///
/// A boxed closure rather than a bare `fn` pointer, so a GPU-backed entry can *capture* a cloned
/// [`GpuContext`]. Every model's factory then has the same shape, and nothing has to thread a
/// context through the app at call time.
///
/// The `Option<u64>` is the RNG seed, which defaults to the model's fixed default when `None`.
pub type ModelFactory = Box<dyn Fn(&[ParamValue], Option<u64>) -> ModelState + Send + Sync>;

/// Captures the same [`GpuContext`] the factory does, so a caller can ask whether a model would
/// build without holding a device of its own.
pub type CapacityFn = Box<dyn Fn(&[ParamValue]) -> Demand + Send + Sync>;

/// An entry in the model registry.
pub struct ModelEntry {
    pub name: String,
    pub id: String,
    pub description: String,
    pub param_descriptors: Vec<ParamDescriptor>,
    pub stat_descriptors: Vec<StatDescriptor>,
    pub topology_hint: TopologyHint,
    pub create: ModelFactory,
    /// `None` for a CPU model, which allocates on the host and has no device limit to miss.
    pub capacity: Option<CapacityFn>,
}

impl ModelEntry {
    /// Reasons this machine cannot build the model at `params`. Empty when nothing stops it.
    pub fn shortfalls(&self, params: &[ParamValue], limits: &wgpu::Limits) -> Vec<String> {
        self.capacity
            .as_ref()
            .map_or_else(Vec::new, |capacity| capacity(params).shortfalls(limits))
    }
}

/// Create a `ModelEntry` from a `GridModel` implementation.
fn register_grid_model<M: GridModel>() -> ModelEntry {
    ModelEntry {
        name: M::NAME.to_owned(),
        id: M::ID.to_owned(),
        description: M::DESCRIPTION.to_owned(),
        param_descriptors: grid_model_param_descriptors::<M>(),
        stat_descriptors: M::STATS.to_vec(),
        topology_hint: TopologyHint::GRID,
        create: Box::new(|params, seed| {
            ModelState::Cpu(Box::new(GridModelState::<M>::from_params_seeded(params, seed)))
        }),
        capacity: None,
    }
}

/// Create a `ModelEntry` from an `AgentModel` implementation.
fn register_agent_model<A: AgentModel>() -> ModelEntry {
    ModelEntry {
        name: A::NAME.to_owned(),
        id: A::ID.to_owned(),
        description: A::DESCRIPTION.to_owned(),
        param_descriptors: agent_model_param_descriptors::<A>(),
        stat_descriptors: A::STATS.to_vec(),
        topology_hint: TopologyHint {
            grid: <A::Field as henad_core::authoring::model::field::FieldLayer>::HAS_GRID,
            agents: true,
        },
        create: Box::new(|params, seed| {
            ModelState::Cpu(Box::new(AgentModelState::<A>::from_params_seeded(params, seed)))
        }),
        capacity: None,
    }
}

/// Create a `ModelEntry` from a `GpuGridModel` implementation, capturing the injected
/// device/queue.
fn register_gpu_grid_model<M: GpuGridModel>(ctx: &GpuContext) -> ModelEntry {
    let model = GpuGridModelDescriptor::<M>::new(ctx.clone());
    let factory_ctx = ctx.clone();
    let capacity_ctx = ctx.clone();
    ModelEntry {
        name: model.name().to_owned(),
        id: model.id().to_owned(),
        description: model.description().to_owned(),
        param_descriptors: model.param_descriptors(),
        stat_descriptors: model.stat_descriptors(),
        topology_hint: model.topology_hint(),
        create: Box::new(move |params, seed| {
            ModelState::Gpu(Box::new(GpuGridState::<M>::new_seeded(&factory_ctx, params, seed)))
        }),
        capacity: Some(Box::new(move |params| {
            GpuGridState::<M>::demand(params, &capacity_ctx.device.limits())
        })),
    }
}

/// Create a `ModelEntry` from a `GpuAgentModel` implementation, capturing the injected
/// device/queue.
fn register_gpu_agent_model<M: GpuAgentModel>(ctx: &GpuContext) -> ModelEntry {
    let model = GpuAgentModelDescriptor::<M>::new(ctx.clone());
    let factory_ctx = ctx.clone();
    let capacity_ctx = ctx.clone();
    ModelEntry {
        name: model.name().to_owned(),
        id: model.id().to_owned(),
        description: model.description().to_owned(),
        param_descriptors: model.param_descriptors(),
        stat_descriptors: model.stat_descriptors(),
        topology_hint: model.topology_hint(),
        create: Box::new(move |params, seed| {
            ModelState::Gpu(Box::new(GpuAgentState::<M>::new_seeded(&factory_ctx, params, seed)))
        }),
        capacity: Some(Box::new(move |params| {
            GpuAgentState::<M>::demand(params, &capacity_ctx.device.limits())
        })),
    }
}

/// Storage buffers the widest pass of any GPU model binds.
///
/// Needed before a device exists, so before there is a [`GpuContext`] to build a registry with.
/// The list below must stay in step with [`model_registry`]'s, which
/// `the_declared_binding_need_matches_the_registry` enforces.
pub fn gpu_storage_bindings_needed() -> u32 {
    [
        GpuGridState::<crate::gpu_game_of_life::GpuGameOfLife>::max_storage_bindings(),
        GpuGridState::<crate::gpu_sir::GpuSir>::max_storage_bindings(),
        GpuAgentState::<crate::gpu_boids::GpuBoids>::max_storage_bindings(),
        GpuAgentState::<crate::gpu_ants::GpuAnts>::max_storage_bindings(),
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
}

/// Every available model.
///
/// GPU-backed models are included only when a [`GpuContext`] is supplied. Without one they are
/// *omitted entirely* rather than listed and then made to fail on selection. A model the user can
/// see in the dropdown should always be one they can actually run.
pub fn model_registry(gpu: Option<GpuContext>) -> Vec<ModelEntry> {
    let mut entries = vec![
        register_grid_model::<crate::sir::SirGridModel>(),
        register_agent_model::<crate::boids::BoidsModel>(),
        register_grid_model::<crate::game_of_life::GameOfLifeModel>(),
        register_agent_model::<crate::ants::AntsModel>(),
    ];

    if let Some(ctx) = gpu {
        entries.push(register_gpu_grid_model::<crate::gpu_game_of_life::GpuGameOfLife>(&ctx));
        entries.push(register_gpu_grid_model::<crate::gpu_sir::GpuSir>(&ctx));
        entries.push(register_gpu_agent_model::<crate::gpu_boids::GpuBoids>(&ctx));
        entries.push(register_gpu_agent_model::<crate::gpu_ants::GpuAnts>(&ctx));
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entry, GPU ones included when this machine can give a device.
    ///
    /// The device asks for `Limits::default()`, so a GPU model that only fits a raised limit fails
    /// to build here. That is deliberate: every model is meant to run on a stock WebGPU device.
    fn all_entries() -> Vec<ModelEntry> {
        model_registry(crate::tests::support::headless_context(
            "registry_test_device",
            wgpu::Features::empty(),
        ))
    }

    fn defaults(entry: &ModelEntry) -> Vec<ParamValue> {
        entry
            .param_descriptors
            .iter()
            .map(|desc| desc.kind.default_value())
            .collect()
    }

    /// Both arms are a `SimState`, which is where the contracts below live.
    fn sim_state(state: &mut ModelState) -> &mut dyn SimState {
        match state {
            ModelState::Cpu(state) => state.as_mut(),
            ModelState::Gpu(state) => state.as_mut(),
        }
    }

    /// The UI labels parameters from the descriptor and the state decides what it accepts, so the
    /// two disagreeing means the panel lies about what an edit does.
    #[test]
    fn declared_apply_mode_matches_what_the_state_accepts() {
        for entry in all_entries() {
            let values = defaults(&entry);
            let mut created = (entry.create)(&values, None);
            let state = sim_state(&mut created);

            for (i, desc) in entry.param_descriptors.iter().enumerate() {
                assert_eq!(
                    state.set_param(i, &values[i]),
                    desc.is_live(),
                    "{}: parameter '{}' is declared {:?} but set_param disagrees",
                    entry.id,
                    desc.id,
                    desc.apply
                );
            }
        }
    }

    /// Nothing else reads `topology_hint`, so without this it drifts from what the state returns.
    #[test]
    fn declared_topology_matches_the_views_the_state_returns() {
        for entry in model_registry(None) {
            let values = defaults(&entry);
            let ModelState::Cpu(state) = (entry.create)(&values, None) else {
                continue;
            };

            assert_eq!(
                state.grid_view().is_some(),
                entry.topology_hint.grid,
                "{}: declares grid={} but grid_view() disagrees",
                entry.id,
                entry.topology_hint.grid
            );
            assert_eq!(
                state.point_view().is_some(),
                entry.topology_hint.agents,
                "{}: declares agents={} but point_view() disagrees",
                entry.id,
                entry.topology_hint.agents
            );
        }
    }

    /// Labels and colours are declared once and paired with values positionally, so a model that
    /// returns too few values loses its trailing series rather than mislabelling anything. Silent
    /// either way, hence this.
    #[test]
    fn every_declared_stat_series_gets_a_value() {
        for entry in all_entries() {
            let values = defaults(&entry);
            let mut created = (entry.create)(&values, None);
            let state = sim_state(&mut created);
            assert_eq!(
                state.stats().len(),
                entry.stat_descriptors.len(),
                "{}: declares {} stat series but produced {} values",
                entry.id,
                entry.stat_descriptors.len(),
                state.stats().len()
            );
        }
    }

    /// The GPU counterpart of the test above. A GPU state publishes through its snapshot rather
    /// than through `grid_view`/`point_view`, so that is what the hint has to agree with.
    #[test]
    fn declared_topology_matches_the_layers_a_gpu_state_publishes() {
        for entry in all_entries() {
            let values = defaults(&entry);
            let ModelState::Gpu(state) = (entry.create)(&values, None) else {
                continue;
            };
            let view = state.view();
            assert_eq!(
                view.display.is_some(),
                entry.topology_hint.grid,
                "{}: declares grid={} but its snapshot disagrees",
                entry.id,
                entry.topology_hint.grid
            );
            assert_eq!(
                view.agents.is_some(),
                entry.topology_hint.agents,
                "{}: declares agents={} but its snapshot disagrees",
                entry.id,
                entry.topology_hint.agents
            );
        }
    }

    #[test]
    fn registry_without_gpu_context_offers_no_gpu_models() {
        let entries = model_registry(None);
        assert!(
            !entries.iter().any(|e| e.id == "gpu_game_of_life" || e.id == "gpu_sir"),
            "a GPU model must not appear in the dropdown when there is no device to run it on"
        );
        assert!(
            entries.iter().any(|e| e.id == "game_of_life"),
            "CPU models must still be registered without a GPU context"
        );
    }

    /// Building every GPU model on a baseline device is what makes "runs on a stock WebGPU
    /// device" a fact rather than an argument: the engine asserts each pass against the device's
    /// own `max_storage_buffers_per_shader_stage`, which is 8 here.
    #[test]
    fn every_gpu_model_builds_on_a_baseline_device() {
        let entries = all_entries();
        let gpu: Vec<&ModelEntry> = entries.iter().filter(|e| e.id.starts_with("gpu_")).collect();
        if gpu.is_empty() {
            log::warn!("skipping every_gpu_model_builds_on_a_baseline_device: no adapter");
            return;
        }
        for entry in gpu {
            let params = defaults(entry);
            // The two pin each other: under-report a pass and the build fails, over-report one
            // and the assert does.
            let _built: ModelState = (entry.create)(&params, None);
            assert!(
                entry.shortfalls(&params, &wgpu::Limits::default()).is_empty(),
                "{}: builds on a baseline device but its declared demand says it should not: {:?}",
                entry.id,
                entry.shortfalls(&params, &wgpu::Limits::default())
            );
        }
    }

    /// The app asks every frame, before building, so a missing capacity is a panic on the UI
    /// thread.
    #[test]
    fn every_gpu_entry_reports_its_capacity() {
        let entries = all_entries();
        let gpu: Vec<&ModelEntry> = entries.iter().filter(|e| e.id.starts_with("gpu_")).collect();
        if gpu.is_empty() {
            log::warn!("skipping every_gpu_entry_reports_its_capacity: no adapter");
            return;
        }
        for entry in gpu {
            let capacity = entry.capacity.as_ref().expect("a GPU entry declares its capacity");
            assert!(
                capacity(&defaults(entry)).bytes() > 0,
                "{}: a GPU model allocates something",
                entry.id
            );
        }
        for entry in entries.iter().filter(|e| !e.id.starts_with("gpu_")) {
            assert!(
                entry.capacity.is_none(),
                "{}: a CPU model has no device demand",
                entry.id
            );
        }
    }

    /// `gpu_storage_bindings_needed` reads a hand-written list of model types. Forget to add a
    /// model to it and the device comes out too narrow, which shows up as a validation error.
    #[test]
    fn the_declared_binding_need_matches_the_registry() {
        let entries = all_entries();
        let gpu: Vec<&ModelEntry> = entries.iter().filter(|e| e.id.starts_with("gpu_")).collect();
        if gpu.is_empty() {
            log::warn!("skipping the_declared_binding_need_matches_the_registry: no adapter");
            return;
        }
        let widest = gpu
            .iter()
            .filter_map(|entry| entry.capacity.as_ref().map(|capacity| capacity(&defaults(entry))))
            .flat_map(|demand| demand.passes.into_iter().map(|pass| pass.storage))
            .max()
            .unwrap_or(0);
        assert_eq!(
            gpu_storage_bindings_needed(),
            widest,
            "a registered GPU model is missing from gpu_storage_bindings_needed's list"
        );
    }

    /// Reported, not built. Otherwise the Build button hands wgpu a bind group it rejects.
    #[test]
    fn a_model_too_large_for_the_device_is_reported() {
        let entries = all_entries();
        let Some(entry) = entries.iter().find(|e| e.id == "gpu_sir") else {
            log::warn!("skipping a_model_too_large_for_the_device_is_reported: no adapter");
            return;
        };
        // Baseline limits, so this is issue #9's 6000x6000 case rather than the machine's.
        let baseline = wgpu::Limits::default();
        let mut params = defaults(entry);
        params[0] = ParamValue::U32(6000);
        params[1] = ParamValue::U32(6000);

        let found = entry.shortfalls(&params, &baseline);
        assert!(
            found.iter().any(|s| s.contains("gpu_sir_buffer0")),
            "the over-budget state buffer must be named: {found:?}"
        );
        assert!(
            entry.shortfalls(&defaults(entry), &baseline).is_empty(),
            "the default params fit a baseline device"
        );
    }
}
