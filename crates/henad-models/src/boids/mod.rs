pub(crate) mod state;
mod step;

use henad_core::helpers::{f32_param, u32_param};
use henad_core::model::Model;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;
use henad_core::view::StatDescriptor;

use crate::boids::state::BoidsState;

/// Boids flocking model in 2D continuous space.
pub struct BoidsModel;

impl Default for BoidsModel {
    fn default() -> Self {
        Self
    }
}

impl Model for BoidsModel {
    type State = BoidsState;

    fn name(&self) -> &'static str {
        "Boids Flocking"
    }

    fn id(&self) -> &'static str {
        "boids"
    }

    fn description(&self) -> &'static str {
        "A simulation of flocking behavior in a group of boids."
    }

    fn param_descriptors(&self) -> Vec<ParamDescriptor> {
        vec![
            u32_param("num_boids", "Number of Boids", 50_000, 1_000, 1_000_000).on_reload(),
            f32_param("world_width", "World Width", 1_000.0, 100.0, 10_000.0, Some(50.0)).on_reload(),
            f32_param("world_height", "World Height", 1_000.0, 100.0, 10_000.0, Some(50.0)).on_reload(),
            f32_param("visual_range", "Visual Range", 50.0, 1.0, 200.0, Some(1.0)),
            f32_param("protected_range", "Protected Range", 8.0, 0.5, 50.0, Some(0.5)),
            f32_param("separation", "Separation", 0.05, 0.0, 2.0, Some(0.01)),
            f32_param("alignment", "Alignment", 0.05, 0.0, 2.0, Some(0.01)),
            f32_param("cohesion", "Cohesion", 0.0005, 0.0, 0.01, Some(0.0001)),
            f32_param("max_speed", "Max Speed", 15.0, 1.0, 50.0, Some(0.5)),
            f32_param("min_speed", "Min Speed", 3.0, 0.5, 20.0, Some(0.5)),
        ]
    }

    fn stat_descriptors(&self) -> Vec<StatDescriptor> {
        vec![
            StatDescriptor {
                label: "Average Speed",
                color: state::PALETTE[1],
            },
            StatDescriptor {
                label: "Average Velocity",
                color: state::PALETTE[2],
            },
        ]
    }

    fn topology_hint(&self) -> TopologyHint {
        TopologyHint::AGENTS
    }

    fn create_state(&self, params: &[ParamValue]) -> Self::State {
        BoidsState::from_params(params)
    }
}
