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

/// Each shader's `@group(0)` declarations, generated alongside the bindings in `build.rs`.
mod binding_decls {
    include!(concat!(env!("OUT_DIR"), "/binding_decls.rs"));
}

pub mod ants;
pub mod boids;
pub mod game_of_life;
pub mod gpu_ants;
pub mod gpu_boids;
pub mod gpu_game_of_life;
pub mod gpu_sir;
pub mod registry;
pub mod sir;

#[cfg(test)]
mod tests;

/// `Dims` is written by the grid engine and read by every grid model's display and reduce shader,
/// so this crate is the only one that sees both the Rust struct and the WGSL it mirrors.
const _: () = {
    use std::mem::offset_of;
    type Rust = henad_compute::gpu::grid_engine::Dims;
    type Wgsl = shader_bindings::shared::dims::Dims;
    assert!(
        size_of::<Rust>() == size_of::<Wgsl>(),
        "Dims size drifted from shared/dims.wgsl"
    );
    assert!(offset_of!(Rust, grid) == offset_of!(Wgsl, grid), "Dims.grid moved");
    assert!(offset_of!(Rust, tex) == offset_of!(Wgsl, tex), "Dims.tex moved");
};
