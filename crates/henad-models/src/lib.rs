//! Example models for the Henad engine.
//!
//! [`example_models`] returns all ten as a [`ModelSet`], the set the app and the CLI offer.

// --8<-- [start:include_shaders]
henad_compute::include_shaders!();
// --8<-- [end:include_shaders]

pub mod ants;
pub mod boids;
pub mod game_of_life;
pub mod gpu_ants;
pub mod gpu_boids;
pub mod gpu_game_of_life;
pub mod gpu_sir;
pub mod sir;
pub mod team_assembly;
pub mod virus_network;

#[cfg(test)]
mod tests;

use henad_compute::entry::{
    ModelSet, register_agent_model, register_gpu_agent_model, register_gpu_grid_model, register_grid_model,
    register_network_model,
};

/// Returns the ten example models, with henad-models' own build as their source.
pub fn example_models() -> ModelSet {
    let entries = [
        // --8<-- [start:cpu_entries]
        register_grid_model::<crate::sir::SirGridModel>(),
        register_agent_model::<crate::boids::BoidsModel>(),
        register_grid_model::<crate::game_of_life::GameOfLifeModel>(),
        register_agent_model::<crate::ants::AntsModel>(),
        register_network_model::<crate::virus_network::VirusNetwork>(),
        register_network_model::<crate::team_assembly::TeamAssembly>(),
        // --8<-- [end:cpu_entries]
        // --8<-- [start:gpu_entries]
        register_gpu_grid_model::<crate::gpu_game_of_life::GpuGameOfLife>(),
        register_gpu_grid_model::<crate::gpu_sir::GpuSir>(),
        register_gpu_agent_model::<crate::gpu_boids::GpuBoids>(),
        register_gpu_agent_model::<crate::gpu_ants::GpuAnts>(),
        // --8<-- [end:gpu_entries]
    ];
    let mut models = ModelSet::new(henad_core::build_info!());
    for entry in entries {
        // The ids are fixed above, and `every_example_model_joins_the_set` checks them.
        models.insert(entry).expect("every example model has an id of its own");
    }
    models
}
