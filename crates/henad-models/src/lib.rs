//! Simulation models for the Henad engine.

pub mod ants;
pub mod boids;
pub mod game_of_life;
pub mod gpu_ants;
pub mod gpu_boids;
pub mod gpu_game_of_life;
pub mod gpu_sir;
#[cfg(test)]
mod gpu_test_support;
pub mod registry;
pub mod sir;
