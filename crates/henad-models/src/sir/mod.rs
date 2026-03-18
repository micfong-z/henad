mod state;
mod step;

use henad_core::{
    model::Model,
    params::{ParamDescriptor, ParamKind, ParamValue},
    topology::TopologyHint,
};

pub use state::SirState;

/// SIR (Susceptible-Infected-Recovered) epidemic model on a 2D grid.
pub struct SirModel;

impl Model for SirModel {
    type State = SirState;

    fn name(&self) -> &'static str {
        "SIR Epidemic"
    }

    fn id(&self) -> &'static str {
        "sir"
    }

    fn description(&self) -> &'static str {
        "Classic SIR compartmental model on a 2D grid with Moore neighborhood"
    }

    fn param_descriptors(&self) -> Vec<ParamDescriptor> {
        vec![
            ParamDescriptor {
                id: "grid_width",
                label: "Grid Width",
                kind: ParamKind::U32 {
                    min: 1,
                    max: 10_000,
                    default: 1024,
                },
            },
            ParamDescriptor {
                id: "grid_height",
                label: "Grid Height",
                kind: ParamKind::U32 {
                    min: 1,
                    max: 10_000,
                    default: 1024,
                },
            },
            ParamDescriptor {
                id: "infection_rate",
                label: "Infection Rate",
                kind: ParamKind::F32 {
                    min: 0.0,
                    max: 1.0,
                    default: 0.3,
                    step: Some(0.01),
                },
            },
            ParamDescriptor {
                id: "recovery_rate",
                label: "Recovery Rate",
                kind: ParamKind::F32 {
                    min: 0.0,
                    max: 1.0,
                    default: 0.05,
                    step: Some(0.01),
                },
            },
            ParamDescriptor {
                id: "initial_infected_pct",
                label: "Initial Infected %",
                kind: ParamKind::F32 {
                    min: 0.0,
                    max: 1.0,
                    default: 0.01,
                    step: Some(0.001),
                },
            },
        ]
    }

    fn topology_hint(&self) -> TopologyHint {
        TopologyHint::Grid2D
    }

    fn create_state(&self, params: &[ParamValue]) -> SirState {
        SirState::from_params(params)
    }
}
