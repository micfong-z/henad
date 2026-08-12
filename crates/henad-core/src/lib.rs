//! Core traits and types for the Henad simulation engine.
//!
//! [`authoring`] is what a model implements, [`model`] is what the runner drives. The rest are the
//! shared data structures and the descriptors the UI reads.

pub mod authoring;
pub mod grid;
pub mod helpers;
pub mod model;
pub mod params;
pub mod spatial_hash;
pub mod topology;
pub mod view;

/// World size, used far too widely to be worth spelling out its authoring path every time.
pub use authoring::field::Extent;
