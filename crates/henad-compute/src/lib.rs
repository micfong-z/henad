//! Engine machinery that turns an authoring impl into something runnable.
//!
//! [`cpu`] and [`gpu`] are siblings, not a base and a specialisation. Each holds its own runner,
//! its own engines, and its own primitives. [`snapshot`], [`runtime_info`], [`display_scale`] and
//! [`fault`] are shared, since both backends publish through them. [`runner`] is how either
//! one gets driven. [`entry`] type-erases a model behind one entry a host can list and build, and
//! [`simulation`] builds one with checked values and steps it from a program.

#![cfg_attr(docsrs, feature(doc_cfg))]
// Proving a type that holds wgpu handles `Send` or `Sync` walks wgpu-core's registries, deeper than the default
// limit of 128.
#![recursion_limit = "256"]

/// Brings in the Rust that henad-build generated from the crate's WGSL, as the modules `shader_bindings` and
/// `binding_decls`.
///
/// Call it once, at the crate root of a crate whose build script runs `henad_build::ShaderBuild`. A shader at
/// `gpu_vote/step.wgsl` below the shader root then reads as `crate::shader_bindings::gpu_vote::step::SHADER_STRING`,
/// and its `@group(0)` declarations as `crate::binding_decls::bindings::GPU_VOTE_STEP`.
///
/// Generated code is not held to the caller's lints, and both modules allow every lint it trips. `unsafe_code` is the
/// one that matters. The generator writes `unsafe impl bytemuck::Pod` and an `unsafe fn from_raw`, so a crate denies
/// `unsafe_code` rather than forbidding it.
///
/// Note that the expansion fails to compile when henad-build composed the shaders against other shared WGSL than this
/// crate links. Use the same 0.x of henad and henad-build.
#[macro_export]
macro_rules! include_shaders {
    () => {
        #[allow(
            unsafe_code,
            dead_code,
            unused_imports,
            unreachable_pub,
            unused_qualifications,
            elided_lifetimes_in_paths,
            single_use_lifetimes,
            clippy::all,
            clippy::pedantic,
            clippy::restriction,
            clippy::nursery
        )]
        mod shader_bindings {
            use $crate::__shader_support::{bytemuck, wgpu};
            ::core::include!(::core::concat!(::core::env!("OUT_DIR"), "/shader_bindings.rs"));
        }

        #[allow(
            unsafe_code,
            dead_code,
            unused_imports,
            unreachable_pub,
            unused_qualifications,
            elided_lifetimes_in_paths,
            single_use_lifetimes,
            clippy::all,
            clippy::pedantic,
            clippy::restriction,
            clippy::nursery
        )]
        mod binding_decls {
            use $crate::__shader_support::{BindingDecl, BindingKind};
            ::core::include!(::core::concat!(::core::env!("OUT_DIR"), "/binding_decls.rs"));
        }

        const _: () = ::core::assert!(
            binding_decls::SHARED_WGSL_FNV1A64 == $crate::__shader_support::SHARED_WGSL_FNV1A64,
            "henad-build composed these shaders against other shared WGSL than this henad links. Use the same 0.x of \
             henad and henad-build, from one source.",
        );
    };
}

include_shaders!();

pub mod cpu;
pub mod display_scale;
pub mod entry;
pub mod fault;
pub mod gpu;
pub mod runner;
pub mod runtime_info;
pub mod simulation;
pub mod snapshot;

/// henad-compute's build, whose source hash covers its own `src` and manifest alone.
#[doc(hidden)]
pub const __COMPUTE_BUILD: henad_core::provenance::BuildInfo = henad_core::build_info!();

/// Items the code [`include_shaders!`] brings in names through `$crate`.
#[doc(hidden)]
pub mod __shader_support {
    pub use henad_core::authoring::model::binding::{BindingDecl, BindingKind};
    pub use henad_core::authoring::primitives::wgsl::SHARED_WGSL_FNV1A64;
    pub use {bytemuck, wgpu};
}

/// Items the exported macros name through `$crate`, so a caller needs none of them in scope.
#[doc(hidden)]
pub mod __macro_support {
    pub use henad_core::authoring::model::agent_model::{AgentLanes, ChunkTally};
    pub use henad_core::metadata::LaneSpec;
    pub use rayon;

    pub use crate::cpu::primitives::chunked::chunk_seed;
}
