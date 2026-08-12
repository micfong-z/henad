use henad_compute::agent_engine::{AgentModelState, agent_model_param_descriptors};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::gpu_grid_engine::{GpuGridModelDescriptor, GpuGridState};
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::grid_engine::{GridModelState, grid_model_param_descriptors};
use henad_core::agent_model::AgentModel;
use henad_core::gpu_grid_model::GpuGridModel;
use henad_core::grid_model::GridModel;
use henad_core::model::{Model as _, SimState};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;
use henad_core::view::StatDescriptor;

/// A freshly created simulation state, tagged with which runner can drive it.
///
/// The two arms are not interchangeable: a CPU state is stepped one tick per call by
/// `henad_compute::sim_thread::SimThread`, while a GPU state has many steps *encoded into one
/// submission* by `henad_compute::gpu::GpuSimThread`. The factory returns this enum (rather than
/// a bare `Box<dyn SimState>`) so the caller can pick the right runner without downcasting, and
/// so it is impossible to hand a GPU state to the CPU thread by mistake.
pub enum ModelState {
    Cpu(Box<dyn SimState>),
    Gpu(Box<dyn GpuSimState>),
}

/// A type-erased model factory.
///
/// A boxed closure rather than a bare `fn` pointer specifically so that a GPU-backed entry can
/// *capture* a cloned [`GpuContext`]. That keeps the factory's shape identical for every model —
/// nobody has to thread a context object through the app at call time — while still letting GPU
/// models reach a device.
///
/// The `Option<u64>` is the RNG seed, which defaults to the model's fixed default when `None`.
pub type ModelFactory = Box<dyn Fn(&[ParamValue], Option<u64>) -> ModelState + Send + Sync>;

/// An entry in the model registry.
pub struct ModelEntry {
    pub name: String,
    pub id: String,
    pub description: String,
    pub param_descriptors: Vec<ParamDescriptor>,
    pub stat_descriptors: Vec<StatDescriptor>,
    pub topology_hint: TopologyHint,
    pub create: ModelFactory,
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
            grid: <A::Field as henad_core::field::FieldLayer>::HAS_GRID,
            agents: true,
        },
        create: Box::new(|params, seed| {
            ModelState::Cpu(Box::new(AgentModelState::<A>::from_params_seeded(params, seed)))
        }),
    }
}

/// Create a `ModelEntry` from a `GpuGridModel` implementation, capturing the injected
/// device/queue.
///
/// The GPU counterpart of [`register_grid_model`]: the factory closure captures its own
/// [`GpuContext`] clone, so callers never thread a device through at creation time.
fn register_gpu_grid_model<M: GpuGridModel>(ctx: &GpuContext) -> ModelEntry {
    let model = GpuGridModelDescriptor::<M>::new(ctx.clone());
    let factory_ctx = ctx.clone();
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
    }
}

/// Create a `ModelEntry` for a the GPU boids model.
fn register_gpu_boids(ctx: &GpuContext) -> ModelEntry {
    let factory_ctx = ctx.clone();
    ModelEntry {
        name: crate::gpu_boids::NAME.to_owned(),
        id: crate::gpu_boids::ID.to_owned(),
        description: crate::gpu_boids::DESCRIPTION.to_owned(),
        param_descriptors: crate::gpu_boids::param_descriptors(),
        stat_descriptors: crate::gpu_boids::stat_descriptors(),
        topology_hint: TopologyHint {
            grid: false,
            agents: true,
        },
        create: Box::new(move |params, seed| {
            ModelState::Gpu(Box::new(crate::gpu_boids::GpuBoidsState::new_seeded(
                &factory_ctx,
                params,
                seed,
            )))
        }),
    }
}

/// Returns all available models.
///
/// GPU-backed models are included only when a [`GpuContext`] is supplied. When it is `None` (no
/// wgpu device — e.g. the web build today, or a headless runner that never acquired one) they are
/// *omitted from the list entirely* rather than listed and then made to fail on selection: a model
/// the user can see in the dropdown should always be one they can actually run.
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
        entries.push(register_gpu_boids(&ctx));
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI labels parameters from the descriptor and the state decides what it accepts, so the
    /// two disagreeing means the panel lies about what an edit does.
    #[test]
    fn declared_apply_mode_matches_what_the_state_accepts() {
        for entry in model_registry(None) {
            let values: Vec<ParamValue> = entry
                .param_descriptors
                .iter()
                .map(|desc| desc.kind.default_value())
                .collect();
            let ModelState::Cpu(mut state) = (entry.create)(&values, None) else {
                continue;
            };

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
            let values: Vec<ParamValue> = entry
                .param_descriptors
                .iter()
                .map(|desc| desc.kind.default_value())
                .collect();
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
        for entry in model_registry(None) {
            let values: Vec<ParamValue> = entry
                .param_descriptors
                .iter()
                .map(|desc| desc.kind.default_value())
                .collect();
            let ModelState::Cpu(state) = (entry.create)(&values, None) else {
                continue;
            };
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
}
