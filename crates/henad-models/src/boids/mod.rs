mod state;
mod step;

use henad_core::{
    model::Model,
    params::{ParamDescriptor, ParamKind, ParamValue},
    topology::TopologyHint,
};

use crate::boids::state::BoidsState;

/// Boids flocking model in 2D continuous space.
pub struct BoidsModel;

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

    #[expect(clippy::too_many_lines, reason = "Parameter definitions are verbose by nature")]
    fn param_descriptors(&self) -> Vec<ParamDescriptor> {
        vec![
            ParamDescriptor {
                id: "num_boids",
                label: "Number of Boids",
                kind: ParamKind::U32 {
                    min: 1_000,
                    max: 1_000_000,
                    default: 50_000,
                },
            },
            ParamDescriptor {
                id: "world_width",
                label: "World Width",
                kind: ParamKind::F32 {
                    min: 100.0,
                    max: 10_000.0,
                    default: 1_000.0,
                    step: Some(50.0),
                },
            },
            ParamDescriptor {
                id: "world_height",
                label: "World Height",
                kind: ParamKind::F32 {
                    min: 100.0,
                    max: 10_000.0,
                    default: 1_000.0,
                    step: Some(50.0),
                },
            },
            ParamDescriptor {
                id: "visual_range",
                label: "Visual Range",
                kind: ParamKind::F32 {
                    min: 1.0,
                    max: 200.0,
                    default: 50.0,
                    step: Some(1.0),
                },
            },
            ParamDescriptor {
                id: "protected_range",
                label: "Protected Range",
                kind: ParamKind::F32 {
                    min: 0.5,
                    max: 50.0,
                    default: 8.0,
                    step: Some(0.5),
                },
            },
            ParamDescriptor {
                id: "separation",
                label: "Separation",
                kind: ParamKind::F32 {
                    min: 0.0,
                    max: 2.0,
                    default: 0.05,
                    step: Some(0.01),
                },
            },
            ParamDescriptor {
                id: "alignment",
                label: "Alignment",
                kind: ParamKind::F32 {
                    min: 0.0,
                    max: 2.0,
                    default: 0.05,
                    step: Some(0.01),
                },
            },
            ParamDescriptor {
                id: "cohesion",
                label: "Cohesion",
                kind: ParamKind::F32 {
                    min: 0.0,
                    max: 0.01,
                    default: 0.0005,
                    step: Some(0.0001),
                },
            },
            ParamDescriptor {
                id: "max_speed",
                label: "Max Speed",
                kind: ParamKind::F32 {
                    min: 1.0,
                    max: 50.0,
                    default: 15.0,
                    step: Some(0.5),
                },
            },
            ParamDescriptor {
                id: "min_speed",
                label: "Min Speed",
                kind: ParamKind::F32 {
                    min: 0.5,
                    max: 20.0,
                    default: 3.0,
                    step: Some(0.5),
                },
            },
        ]
    }

    fn topology_hint(&self) -> TopologyHint {
        TopologyHint::PointCloud
    }

    fn create_state(&self, params: &[ParamValue]) -> Self::State {
        BoidsState::from_params(params)
    }
}
