//! Core traits and types for the Henad simulation engine.
//!
//! [`authoring`] is what a model implements, [`model`] is what the runner drives. The rest are the
//! shared data structures and the descriptors the UI reads.

pub mod authoring;
pub mod grid;
pub mod helpers;
pub mod model;
pub mod params;
pub mod send_sync;
pub mod spatial_hash;
pub mod topology;
pub mod view;

/// World size.
pub use authoring::model::field::Extent;
