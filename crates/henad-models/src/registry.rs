use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::grid_engine::{GridModelState, grid_model_param_descriptors};
use henad_core::grid_model::GridModel;
use henad_core::model::{Model, SimState};
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
pub type ModelFactory = Box<dyn Fn(&[ParamValue]) -> ModelState + Send + Sync>;

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
        stat_descriptors: M::stat_descriptors(),
        topology_hint: TopologyHint::Grid2D,
        create: Box::new(|params| {
            ModelState::Cpu(Box::new(GridModelState::<M>::from_params(params)))
        }),
    }
}

/// Create a `ModelEntry` from a full `Model` implementation.
fn register_full_model<M: Model + Default>() -> ModelEntry
where
    M::State: SimState,
{
    let m = M::default();
    ModelEntry {
        name: m.name().to_owned(),
        id: m.id().to_owned(),
        description: m.description().to_owned(),
        param_descriptors: m.param_descriptors(),
        stat_descriptors: m.stat_descriptors(),
        topology_hint: m.topology_hint(),
        create: Box::new(|params| ModelState::Cpu(Box::new(M::default().create_state(params)))),
    }
}

/// Create a `ModelEntry` for the GPU Game of Life, capturing the injected device/queue.
fn register_gpu_game_of_life(ctx: &GpuContext) -> ModelEntry {
    let model = crate::gpu_game_of_life::GpuGameOfLifeModel::new(ctx.clone());
    let factory_ctx = ctx.clone();
    ModelEntry {
        name: model.name().to_owned(),
        id: model.id().to_owned(),
        description: model.description().to_owned(),
        param_descriptors: model.param_descriptors(),
        stat_descriptors: model.stat_descriptors(),
        topology_hint: model.topology_hint(),
        create: Box::new(move |params| {
            ModelState::Gpu(Box::new(crate::gpu_game_of_life::GpuGameOfLifeState::new(
                &factory_ctx,
                params,
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
        register_full_model::<crate::boids::BoidsModel>(),
        register_grid_model::<crate::game_of_life::GameOfLifeModel>(),
    ];

    if let Some(ctx) = gpu {
        entries.push(register_gpu_game_of_life(&ctx));
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_without_gpu_context_offers_no_gpu_models() {
        let entries = model_registry(None);
        assert!(
            !entries.iter().any(|e| e.id == "gpu_game_of_life"),
            "a GPU model must not appear in the dropdown when there is no device to run it on"
        );
        assert!(
            entries.iter().any(|e| e.id == "game_of_life"),
            "CPU models must still be registered without a GPU context"
        );
    }
}
