//! Core traits and types for the Henad simulation engine.
//!
//! [`authoring`] is what a model implements, [`model`] is what the runner drives. The rest are the
//! shared data structures and the descriptors the UI reads.

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

/// Items the exported macros name through `$crate`, so a caller needs none of them in scope.
#[doc(hidden)]
pub mod __macro_support {
    pub use crate::action::ActionDescriptor;
    pub use crate::authoring::model::gpu_agent_model::BufferSpec;
    pub use crate::params::ParamDescriptor;
    pub use crate::provenance::BuildInfo;
}
