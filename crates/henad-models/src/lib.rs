//! Simulation models for the Henad engine.

/// Rust generated from this crate's WGSL by `wgsl_bindgen`, in `build.rs`.
///
/// Generated code is not held to the workspace's lints, hence the group allows. `unsafe_code` is
/// the one that matters. The generator writes `unsafe impl bytemuck::Pod` and an
/// `unsafe fn from_raw`, so the workspace deny is lifted here and nowhere else.
#[allow(
    unsafe_code,
    dead_code,
    elided_lifetimes_in_paths,
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery
)]
mod shader_bindings {
    include!(concat!(env!("OUT_DIR"), "/shader_bindings.rs"));
}

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
