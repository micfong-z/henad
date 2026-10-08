//! Core traits and types for the Henad simulation engine.
//!
//! - [`authoring`] holds the five model traits, one per topology and backend, and the primitives their
//!   kernels call.
//! - [`params`](mod@params), [`view`] and [`action`] hold the descriptors a model declares for its
//!   parameters, its stats and its one-off actions.
//! - [`model`] holds [`SimState`](model::SimState), the interface the runner drives.
//! - [`explore`] plans sweeps and searches without an engine, and [`export`] writes a run's results.
//! - [`provenance`] identifies the build of each crate and the source of each model.
//!
//! A program depends on the `henad` facade instead. It re-exports these items under one module tree.
//! The [authoring guide](https://micfong-z.github.io/henad/authoring/) explains each trait.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

pub mod action;
pub mod authoring;
pub mod explore;
pub mod export;
pub mod grid;
pub mod helpers;
pub mod metadata;
pub mod model;
pub mod network;
pub mod params;
pub mod provenance;
pub mod send_sync;
pub mod spatial_hash;
pub mod topology;
pub mod view;

/// World size.
pub use authoring::model::field::Extent;

/// Version of henad-core. The crate has no build script to stamp a [`provenance::BuildInfo`].
#[doc(hidden)]
pub const __VERSION: &str = env!("CARGO_PKG_VERSION");

/// Items that the exported macros reference through `$crate`, so a caller needs none of them in scope.
#[doc(hidden)]
pub mod __macro_support {
    pub use crate::action::ActionDescriptor;
    pub use crate::authoring::model::gpu_agent_model::BufferSpec;
    pub use crate::params::ParamDescriptor;
    pub use crate::provenance::BuildInfo;
}
