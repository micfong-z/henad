//! Engine machinery that turns an authoring impl into something runnable.
//!
//! [`cpu`] and [`gpu`] are siblings, not a base and a specialisation. Each holds its own runner,
//! its own engines, and its own primitives. [`snapshot`], [`runtime_info`], [`display_scale`] and
//! [`fault`] are shared, since both backends publish through them.

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
pub mod shader_bindings {
    include!(concat!(env!("OUT_DIR"), "/shader_bindings.rs"));
}

pub mod cpu;
pub mod display_scale;
pub mod fault;
pub mod gpu;
pub mod runtime_info;
pub mod snapshot;

pub use cpu::primitives::lanes_macro::__lanes;
