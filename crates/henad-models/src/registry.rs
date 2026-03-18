use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;

use crate::boids::BoidsModel;
use crate::sir::SirModel;

use henad_core::model::Model as _;

/// An entry in the model registry with type-erased factory.
pub struct ModelEntry {
    pub name: String,
    pub id: String,
    pub description: String,
    pub param_descriptors: Vec<ParamDescriptor>,
    pub topology_hint: TopologyHint,
    pub create: fn(&[ParamValue]) -> Box<dyn SimState>,
}

fn create_sir(params: &[ParamValue]) -> Box<dyn SimState> {
    Box::new(SirModel.create_state(params))
}

fn create_boids(params: &[ParamValue]) -> Box<dyn SimState> {
    Box::new(crate::boids::BoidsModel.create_state(params))
}

/// Returns all available models.
pub fn model_registry() -> Vec<ModelEntry> {
    let sir = SirModel;
    let boids = BoidsModel;
    vec![
        ModelEntry {
            name: sir.name().to_owned(),
            id: sir.id().to_owned(),
            description: sir.description().to_owned(),
            param_descriptors: sir.param_descriptors(),
            topology_hint: sir.topology_hint(),
            create: create_sir,
        },
        ModelEntry {
            name: boids.name().to_owned(),
            id: boids.id().to_owned(),
            description: boids.description().to_owned(),
            param_descriptors: boids.param_descriptors(),
            topology_hint: boids.topology_hint(),
            create: create_boids,
        },
    ]
}
