pub(crate) mod state;
mod step;

use henad_core::helpers::{f32_param, u32_param};
use henad_core::model::Model;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;
use henad_core::view::StatDescriptor;

use crate::ants::state::{AntsState, STAT_PALETTE};

/// Ant foraging. See [`AntsState`] for the divergences from the reference.
///
/// Defaults match it apart from the population, which is 100 there and leaves the field almost
/// empty. Use `--set num_ants=100` for a reference-identical run.
pub struct AntsModel;

impl Default for AntsModel {
    fn default() -> Self {
        Self
    }
}

impl Model for AntsModel {
    type State = AntsState;

    fn name(&self) -> &'static str {
        "Ant Foraging"
    }

    fn id(&self) -> &'static str {
        "ants"
    }

    fn description(&self) -> &'static str {
        "Ants lay and follow pheromone trails between a nest and a food source, around obstacles"
    }

    fn param_descriptors(&self) -> Vec<ParamDescriptor> {
        vec![
            u32_param("grid_width", "Grid Width", 200, 8, 4_096).on_reload(),
            u32_param("grid_height", "Grid Height", 200, 8, 4_096).on_reload(),
            u32_param("num_ants", "Number of Ants", 2_000, 1, 5_000_000).on_reload(),
            f32_param("evaporation", "Evaporation", 0.999, 0.9, 1.0, Some(0.001)),
            f32_param("update_cutdown", "Trail Falloff", 0.9, 0.5, 1.0, Some(0.01)),
            f32_param("reward", "Site Reward", 1.0, 0.1, 10.0, Some(0.1)),
            f32_param("momentum", "Momentum Probability", 0.8, 0.0, 1.0, Some(0.01)),
            f32_param("random_action", "Random Action Probability", 0.1, 0.0, 1.0, Some(0.01)),
        ]
    }

    fn stat_descriptors(&self) -> Vec<StatDescriptor> {
        vec![
            StatDescriptor {
                label: "Carrying Food",
                color: STAT_PALETTE[0],
            },
            StatDescriptor {
                label: "Deliveries",
                color: STAT_PALETTE[1],
            },
            StatDescriptor {
                label: "Total Pheromone",
                color: STAT_PALETTE[2],
            },
        ]
    }

    fn topology_hint(&self) -> TopologyHint {
        TopologyHint::COMPOSITE
    }

    fn create_state(&self, params: &[ParamValue]) -> Self::State {
        AntsState::from_params(params)
    }
}
