use henad_compute::grid_engine::{GridModelState, grid_model_param_descriptors};
use henad_core::grid_model::GridModel;
use henad_core::model::{Model, SimState};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;
use henad_core::view::StatDescriptor;

/// An entry in the model registry with type-erased factory.
pub struct ModelEntry {
    pub name: String,
    pub id: String,
    pub description: String,
    pub param_descriptors: Vec<ParamDescriptor>,
    pub stat_descriptors: Vec<StatDescriptor>,
    pub topology_hint: TopologyHint,
    pub create: fn(&[ParamValue]) -> Box<dyn SimState>,
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
        create: |params| Box::new(GridModelState::<M>::from_params(params)),
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
        create: |params| Box::new(M::default().create_state(params)),
    }
}

/// Returns all available models.
pub fn model_registry() -> Vec<ModelEntry> {
    vec![
        register_grid_model::<crate::sir::SirGridModel>(),
        register_full_model::<crate::boids::BoidsModel>(),
        register_grid_model::<crate::game_of_life::GameOfLifeModel>(),
    ]
}
